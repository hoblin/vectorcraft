//! `vectorcraft-cli`: VectorCraft from the command line. The subcommands and their flags are
//! documented where the agent reads them: the help pages in [`help`].
// Denied, not forbidden: the font listers shared with the desktop app allow it for their
// DirectWrite (Windows) and CoreText (macOS) calls, and nothing else may.
#![deny(unsafe_code)]

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

/// `println!` / `print!` that end the program quietly when stdout is closed
/// (`vectorcraft-cli commands | head`) instead of panicking with "failed printing to stdout:
/// Broken pipe (os error 32)".
macro_rules! outln {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        if let Err(e) = writeln!(std::io::stdout(), $($arg)*) {
            $crate::stdout_failed(e);
        }
    }};
}
macro_rules! out {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        if let Err(e) = write!(std::io::stdout(), $($arg)*) {
            $crate::stdout_failed(e);
        }
    }};
}

mod help;
/// The desktop app's CoreText font lister, shared: exports and MCP find the same fonts.
#[cfg(target_os = "macos")]
#[path = "../../vectorcraft/src/mac_fonts.rs"]
mod mac_fonts;
mod perf;
/// The desktop app's settings folder, shared: exports and MCP read the same Fonts folder.
#[path = "../../vectorcraft/src/prefs_dir.rs"]
mod prefs_dir;
/// The desktop app's DirectWrite font lister, shared: exports and MCP find the same fonts.
#[cfg(all(windows, not(target_vendor = "win7")))]
#[path = "../../vectorcraft/src/system_fonts.rs"]
mod system_fonts;

use serde_json::{Value, json};
use vectorcraft_engine::file_access::{self, AutomationRoots};
use vectorcraft_mcp::{Backend, DEFAULT_ADDR, Headless, Remote, Server};

/// stdout went away. A reader that stopped early (a closed pipe) ends the program quietly, as
/// ripgrep does; any other write error is reported.
fn stdout_failed(e: std::io::Error) -> ! {
    if e.kind() == std::io::ErrorKind::BrokenPipe {
        std::process::exit(0);
    }
    eprintln!("vectorcraft-cli: can't write to stdout: {e}");
    std::process::exit(1);
}

fn main() -> ExitCode {
    // Before any font lookup: the fonts font services load (#579).
    #[cfg(all(windows, not(target_vendor = "win7")))]
    system_fonts::install();
    #[cfg(target_os = "macos")]
    mac_fonts::install();
    prefs_dir::install_fonts();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let first = args.first().map(String::as_str);
    // `SUB --help` explains SUB and does nothing else, whatever SUB would make of `--help`.
    let wants_help = args.get(1).is_some_and(|a| matches!(a.as_str(), "--help" | "-h"));
    let r = match first {
        Some("help") => match args.get(1).map(String::as_str) {
            None => show(help::overview()),
            Some(sub) => help::page(sub).map_or_else(|| Err(unknown_subcommand(sub)), show),
        },
        Some(sub) if wants_help => help::page(sub).map_or_else(|| Err(unknown_subcommand(sub)), show),
        Some("mcp") => mcp(&args[1..]),
        Some("run") => run(&args[1..]),
        Some("commands") => commands(args.get(1).map(String::as_str)),
        Some("describe") => describe(&args[1..]),
        Some("convert") => convert(&args[1..]),
        Some("info") => info(&args[1..]),
        Some("bench") => bench(&args[1..]),
        Some("perf") => perf::run(&args[1..]),
        Some("-h" | "--help") | None => show(help::overview()),
        Some("-V" | "--version") => show(format!("vectorcraft-cli {}\n", env!("CARGO_PKG_VERSION"))),
        Some(other) => Err(unknown_subcommand(other)),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("vectorcraft-cli: {e}");
            ExitCode::FAILURE
        }
    }
}

fn show(page: String) -> Result<(), String> {
    out!("{page}");
    Ok(())
}

fn unknown_subcommand(name: &str) -> String {
    format!("`{name}` is not a subcommand.\n\n{}", help::overview())
}

fn mcp(args: &[String]) -> Result<(), String> {
    let mut connect: Option<String> = None;
    let mut headless = false;
    let (mut read_root, mut write_root): (Option<String>, Option<String>) = (None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        // The roots also come as `--flag=DIR`, as the other Craft apps take them.
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) if f.starts_with("--automation-") => (f, Some(v.to_string())),
            _ => (a.as_str(), None),
        };
        match flag {
            "--connect" => connect = Some(it.next().cloned().ok_or("--connect needs an address")?),
            "--headless" => headless = true,
            "--automation-read-root" | "--automation-write-root" => {
                let dir = inline.or_else(|| it.next().cloned()).ok_or_else(|| format!("{flag} needs a folder"))?;
                let slot = if flag == "--automation-read-root" { &mut read_root } else { &mut write_root };
                if slot.replace(dir).is_some() {
                    return Err(format!("{flag} is given twice: it takes one folder"));
                }
            }
            other => {
                return Err(format!(
                    "mcp was given `{other}`, which it doesn't take. mcp takes --connect ADDR or --headless, --automation-read-root DIR and --automation-write-root DIR"
                ));
            }
        }
    }
    if headless && connect.is_some() {
        return Err("use either --connect or --headless".into());
    }
    let roots = AutomationRoots::new(read_root.as_deref().map(Path::new), write_root.as_deref().map(Path::new))?;
    if roots.is_some() && connect.is_some() {
        return Err("--automation-read-root and --automation-write-root confine the headless server; a running app enforces its own roots:                     start it with `vectorcraft --control <port> --automation-read-root <dir> --automation-write-root <dir>`"
            .into());
    }
    let backend: Box<dyn Backend> = if let Some(roots) = roots {
        // Every thread of this process, not only the server's: nothing reaches past the roots.
        file_access::confine_process(roots.clone());
        let show = |root: Option<&Path>| root.map_or_else(|| "none".to_string(), |r| r.display().to_string());
        eprintln!("vectorcraft-cli: files confined: read root {}, write root {}", show(roots.read_root()), show(roots.write_root()));
        Box::new(Headless::with_document().with_automation_roots(Some(roots)))
    } else if headless {
        Box::new(Headless::with_document())
    } else if let Some(addr) = connect {
        // Explicit address: fail loudly if the app isn't there.
        Box::new(Remote::connect(&addr).map_err(|e| format!("cannot connect to {addr}: {e}"))?)
    } else {
        match Remote::connect(DEFAULT_ADDR) {
            Ok(r) => Box::new(r),
            Err(_) => Box::new(Headless::with_document()),
        }
    };
    eprintln!("vectorcraft-cli: MCP server on stdio ({})", backend.describe());
    // The binary owns the logger, not the library: installing one here keeps an embedder that
    // uses `vectorcraft_mcp` free to bring its own. Silent until a client sends logging/setLevel.
    vectorcraft_mcp::logging::install();
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    Server::new(backend).serve(stdin.lock(), stdout.lock()).map_err(|e| e.to_string())
}

/// One line per command, `id — label: params`, sorted by id and grouped by its first segment;
/// `filter` keeps the commands whose id or label contains it, ignoring case.
fn commands(filter: Option<&str>) -> Result<(), String> {
    let mut h = Headless::new();
    let v = h.call("engine.commands", json!({}))?;
    let lowered = filter.map(str::to_ascii_lowercase);
    let mut lines: Vec<(String, String)> = v
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let id = c["id"].as_str()?;
            let label = c["label"].as_str().unwrap_or_default();
            let keep = lowered.as_ref().is_none_or(|f| id.to_ascii_lowercase().contains(f) || label.to_ascii_lowercase().contains(f));
            keep.then(|| (id.to_string(), command_line(id, label, c["params"].as_str().unwrap_or_default())))
        })
        .collect();
    lines.sort();
    let mut area = "";
    for (id, line) in &lines {
        let prefix = id.split('.').next().unwrap_or_default();
        if !area.is_empty() && prefix != area {
            outln!();
        }
        area = prefix;
        outln!("{line}");
    }
    if lines.is_empty()
        && let Some(f) = filter
    {
        return Err(format!("no command's id or name contains `{}`; `vectorcraft-cli commands` lists them all", shown(f)));
    }
    Ok(())
}

/// `format.size — Font Size: {size: pt}`, the params as their key outline plus what the command
/// answers (its full description is `describe`'s).
fn command_line(id: &str, label: &str, doc: &str) -> String {
    use vectorcraft_engine::cmd::params::{outline, parts};
    let (_, answer) = parts(doc);
    let mut line = format!("{id} — {label}");
    match outline(doc).as_str() {
        "{}" => {}
        keys => line.push_str(&format!(": {keys}")),
    }
    if let Some(answer) = answer {
        let short: String = answer.split([';']).next().unwrap_or_default().chars().take(80).collect();
        line.push_str(&format!(" → {}{}", short.trim(), if short.len() < answer.len() { "…" } else { "" }));
    }
    line
}

/// One command spelled out: its name and menu, its parameters in full, what it answers, what it
/// acts on, and the `run` call.
fn describe(args: &[String]) -> Result<(), String> {
    use vectorcraft_engine::cmd::params::{outline, parts};
    let [id] = args else {
        return Err(match args {
            [] => "describe takes one command id, e.g. `vectorcraft-cli describe shape.rectangle`; `vectorcraft-cli commands` lists them".to_string(),
            more => format!(
                "describe takes one command id; it was given {}: {}",
                more.len(),
                more.iter().map(|a| shown(a)).collect::<Vec<_>>().join(", ")
            ),
        });
    };
    let (label, doc) = vectorcraft_mcp::command_doc(id)
        .ok_or_else(|| format!("{}. `vectorcraft-cli commands` lists them.", vectorcraft_engine::unknown_command(id)))?;
    let menu = vectorcraft_engine::find_command(id).map(|c| c.menu).filter(|m| !m.is_empty());
    let (params, answer) = parts(doc);
    let (params, note) = match params.rsplit_once('}') {
        Some((head, tail)) if !tail.trim().is_empty() => (format!("{head}}}"), Some(tail.trim())),
        _ => (params.to_string(), None),
    };
    outln!("{id} — {label}{}", menu.map(|m| format!(", in the {} menu", m.join(" › "))).unwrap_or_default());
    outln!();
    match params.as_str() {
        "{}" => outln!("Takes no parameters."),
        p => outln!("Parameters: {p}"),
    }
    if let Some(note) = note {
        outln!("{}", note.trim_start_matches([':', '=', ' ']));
    }
    if let Some(answer) = answer {
        outln!("Answers: {answer}");
    }
    if let Some(acts) = acts_on(id, doc).or_else(|| acts_on_given(doc).map(str::to_string)) {
        outln!("{acts}");
    }
    outln!();
    match outline(doc).as_str() {
        "{}" => outln!("Run it with: vectorcraft-cli run --cmd {id}"),
        _ => outln!("Run it with: vectorcraft-cli run --cmd {id} --params '{{…}}'"),
    }
    Ok(())
}

/// What a command that needs a selection acts on: found by asking the engine, with a document
/// open and nothing selected, why the command isn't available.
fn acts_on(id: &str, doc: &'static str) -> Option<String> {
    use vectorcraft_engine::cmd::params::takes_key;
    let h = Headless::with_document();
    let reason = h.session.commands().into_iter().find(|c| c.id == id)?.disabled_reason?;
    let on = match reason.as_str() {
        "nothing selected" => "the selected objects",
        "select at least two objects" => "two or more selected objects",
        "no anchor points selected" => "the selected anchor points",
        _ => return None,
    };
    let given = if takes_key(doc, "ids") {
        ", or on `ids`"
    } else if takes_key(doc, "id") {
        ", or on `id`"
    } else {
        ""
    };
    Some(format!("Acts on {on}{given}."))
}

/// What a command that takes `ids` (or `id`) as an option, without needing a selection, acts on.
/// A command that requires the id acts on that alone, and says nothing here.
fn acts_on_given(doc: &'static str) -> Option<&'static str> {
    use vectorcraft_engine::cmd::params::optional_key;
    if optional_key(doc, "ids") {
        Some("Acts on `ids`, else on the selected objects.")
    } else if optional_key(doc, "id") {
        Some("Acts on `id`, else on the selected objects.")
    } else {
        None
    }
}

fn convert(args: &[String]) -> Result<(), String> {
    let mut files = vec![];
    let (mut scale, mut artboard, mut range, mut outline_text, mut replace_lossy, mut editing_data) =
        (1.0f64, None::<u64>, None::<String>, false, false, false);
    let mut decimals = None::<u64>;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--scale" | "-s" => scale = it.next().and_then(|v| v.parse().ok()).ok_or("--scale needs a number")?,
            "--artboard" | "-a" => artboard = Some(it.next().and_then(|v| v.parse().ok()).ok_or("--artboard needs an index")?),
            "--range" | "-r" => range = Some(it.next().cloned().ok_or("--range needs artboards such as 1-3,5")?),
            "--outline-text" => outline_text = true,
            "--decimals" => decimals = Some(it.next().and_then(|v| v.parse::<u64>().ok()).ok_or("--decimals needs a number")?),
            "--replace-lossy" => replace_lossy = true,
            "--editing-data" => editing_data = true,
            f => files.push(f.to_string()),
        }
    }
    let [input, output] = <[String; 2]>::try_from(files).map_err(|files: Vec<String>| match files.as_slice() {
        [] => "convert takes IN and OUT, the file to read and the file to write, e.g. convert logo.ai logo.svg".to_string(),
        [one] => format!("convert takes IN and OUT, the file to read and the file to write; it was given only `{one}`"),
        many => format!(
            "convert takes IN and OUT, then options; it was given {} files: {}. convert's options are --scale N, --artboard I, --range R, --outline-text, --decimals N, --replace-lossy and --editing-data",
            many.len(),
            many.join(", ")
        ),
    })?;
    let mut h = Headless::new();
    if editing_data {
        let params = json!({"path": input, "editingData": "always"});
        h.call("engine.execute", json!({"command": "document.open", "params": params})).map_err(|e| format!("open {input}: {e}"))?;
    } else {
        h.call("app.open", json!({"path": input})).map_err(|e| format!("open {input}: {e}"))?;
    }
    // A conversion keeps hidden layers and objects, written hidden, where the format can (SVG,
    // PSD).
    let mut params = json!({"path": output, "scale": scale, "artboard": artboard, "range": range, "outlineText": outline_text, "hiddenLayers": true, "acknowledgeLoss": replace_lossy});
    if let Some(decimals) = decimals {
        params["decimals"] = json!(decimals);
    }
    let r = h.call("engine.execute", json!({"command": "document.export", "params": params})).map_err(|e| format!("export {output}: {e}"))?;
    outln!("{r}");
    Ok(())
}

fn info(args: &[String]) -> Result<(), String> {
    let [file] = args else {
        return Err(match args {
            [] => "info takes FILE, the document to describe, e.g. info logo.ai".to_string(),
            more => format!("info takes one FILE; it was given {}: {}", more.len(), more.iter().map(|a| shown(a)).collect::<Vec<_>>().join(", ")),
        });
    };
    let mut h = Headless::new();
    let opened = h.call("app.open", json!({"path": file})).map_err(|e| format!("open {file}: {e}"))?;
    let base = h.call("engine.execute", json!({"command": "file.info", "params": {}}))?;
    let doc = h.session.doc().map_err(|e| e.to_string())?.doc.clone();
    let mut kinds: std::collections::BTreeMap<&'static str, usize> = Default::default();
    doc.walk(|n| *kinds.entry(n.kind_label()).or_default() += 1);
    let fonts = h.call("engine.execute", json!({"command": "text.fonts", "params": {}})).unwrap_or(Value::Null);
    let artboards: Vec<Value> =
        doc.artboards.iter().map(|a| json!({"name": a.name, "rect": [a.rect.x0, a.rect.y0, a.rect.width(), a.rect.height()]})).collect();
    // What didn't come in as it was (an EPS read from its preview says why).
    let v = json!({"file": file, "warnings": opened["warnings"], "info": base, "artboards": artboards, "kinds": kinds, "fonts": fonts});
    outln!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
    Ok(())
}

enum Step {
    Cmd(String, Value),
    Export(String),
}

/// `text`, at most 200 characters, for an error that echoes what was typed.
fn shown(text: &str) -> String {
    let short: String = text.chars().take(200).collect();
    if short.len() < text.len() { format!("{short}…") } else { short }
}

/// What run takes, for an error about what it was given.
const RUN_TAKES: &str = "run takes --in FILE, --cmd COMMAND, --params JSON (right after its --cmd), --export OUT, --scale N and --print";

/// The flags of `run`, every one read before anything runs, so a mistake in one means nothing ran.
fn run_args(args: &[String]) -> Result<(Option<String>, Vec<Step>, f64, bool), String> {
    use vectorcraft_engine::cmd::params::{outline, takes};
    let mut input: Option<String> = None;
    let mut steps: Vec<Step> = vec![];
    let (mut scale, mut print) = (1.0, false);
    let mut it = args.iter();
    let cmd_count = args.iter().filter(|a| matches!(a.as_str(), "--cmd" | "-c")).count();
    let mut cmds = 0;
    while let Some(a) = it.next() {
        let mut val = |what: &str| it.next().cloned().ok_or_else(|| format!("{a} needs {what}"));
        match a.as_str() {
            "--in" | "-i" => input = Some(val("a file, the document to open")?),
            "--cmd" | "-c" => {
                let id = val("a command id, e.g. --cmd shape.rectangle")?;
                cmds += 1;
                if vectorcraft_mcp::command_doc(&id).is_none() {
                    let like = vectorcraft_engine::similar_commands(&id);
                    let next = if id.starts_with('{') {
                        " --cmd takes a command id; its parameters go in --params after it".to_string()
                    } else if like.is_empty() {
                        " `vectorcraft-cli commands` lists the commands".to_string()
                    } else {
                        format!(" Commands named like it: {}; `vectorcraft-cli commands` lists them all", like.join(", "))
                    };
                    return Err(format!("--cmd {cmds} of {cmd_count} was read as command `{}`, which is not a command.{next}", shown(&id)));
                }
                steps.push(Step::Cmd(id, json!({})));
            }
            "--params" | "-p" => {
                let raw = val("a JSON object, the parameters of the --cmd before it")?;
                let Some(Step::Cmd(id, params)) = steps.last_mut() else {
                    return Err(format!("--params `{}` follows no --cmd: --params gives the parameters of the --cmd right before it", shown(&raw)));
                };
                let at = format!("--cmd {cmds} of {cmd_count} (`{id}`)");
                let takes = vectorcraft_mcp::command_doc(id).map(|(_, doc)| takes(id, &outline(doc))).unwrap_or_default();
                let p: Value = serde_json::from_str(&raw)
                    .map_err(|e| format!("{at} was given --params `{}`, which is not JSON: {e}. {takes}, as one JSON object", shown(&raw)))?;
                if !p.is_object() {
                    return Err(format!("{at} was given --params `{}`, which is not a JSON object. {takes}, as one JSON object", shown(&raw)));
                }
                *params = p;
            }
            "--export" | "-o" => steps.push(Step::Export(val("a file to write, named with its format's extension")?)),
            "--scale" | "-s" => {
                let raw = val("a number, pixels per point")?;
                scale = raw
                    .parse::<f64>()
                    .map_err(|_| format!("--scale was given `{}`, which is not a number: it takes pixels per point, e.g. --scale 2", shown(&raw)))?;
            }
            "--print" => print = true,
            other => return Err(format!("run was given `{}`, which it doesn't take. {RUN_TAKES}", shown(other))),
        }
    }
    Ok((input, steps, scale, print))
}

fn run(args: &[String]) -> Result<(), String> {
    let (input, steps, scale, print) = run_args(args).map_err(|e| format!("{e}. Nothing ran."))?;
    let mut h = Headless::new();
    let mut out = std::io::stdout().lock();
    // A reader that stopped early (a closed pipe) isn't an error, and the batch goes on: its later
    // steps (an export…) still run, unprinted.
    let mut closed = false;
    let mut emit = |line: String| {
        if closed {
            return Ok(());
        }
        match writeln!(out, "{line}") {
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                closed = true;
                Ok(())
            }
            r => r.map_err(|e| e.to_string()),
        }
    };
    let mut done = vec![];
    if let Some(path) = &input {
        let r = h.call("app.open", json!({"path": path})).map_err(|e| format!("--in {path}: {e}. Nothing ran."))?;
        let warnings: Vec<&str> = r["warnings"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        if !warnings.is_empty() {
            emit(format!("opened {path} with warnings: {}", warnings.join("; ")))?;
        } else if print {
            emit(format!("opened {path}"))?;
        }
        done.push(format!("opened {path}"));
    } else if !matches!(steps.first(), Some(Step::Cmd(id, _)) if id == "file.new") {
        h.ensure_document();
    }
    let total = steps.iter().filter(|s| matches!(s, Step::Cmd(..))).count();
    // What a failing step leaves undone: the exports after it, named, so the agent knows what
    // isn't on disk.
    let left_undone = |from: usize| -> String {
        let exports: Vec<&str> =
            steps.get(from..).into_iter().flatten().filter_map(|s| if let Step::Export(p) = s { Some(p.as_str()) } else { None }).collect();
        match (steps.len() > from, exports.is_empty()) {
            (false, _) => String::new(),
            (true, true) => " Nothing after it ran.".into(),
            (true, false) => format!(" Nothing after it ran: {} not written.", exports.join(", ")),
        }
    };
    // Each step's result, for later steps to refer to (`"$1.id"`, see `vectorcraft_engine::steps`).
    let mut results = vec![];
    let mut shown_answers = 0;
    let (mut n, mut exported) = (0, vec![]);
    for (i, step) in steps.iter().enumerate() {
        let r = match step {
            Step::Cmd(id, params) => {
                n += 1;
                let at = format!("--cmd {n} of {total} (`{id}`)");
                let params = vectorcraft_engine::steps::resolve(params, &results).map_err(|e| format!("{at}: {e}.{}", left_undone(i + 1)))?;
                let r = h
                    .call("engine.execute", json!({"command": id, "params": params}))
                    .map_err(|e| format!("{at}: {}.{}", e.trim_end_matches('.'), left_undone(i + 1)))?;
                if print {
                    match &r {
                        Value::String(said) => emit(format!("{id} → {said}"))?,
                        r => emit(format!("{id} → {r}"))?,
                    }
                } else if !r.is_string() {
                    shown_answers += 1;
                }
                r
            }
            Step::Export(path) => {
                let r = h
                    .call("app.export", json!({"path": path, "scale": scale}))
                    .map_err(|e| format!("--export {path}: {}.{}", e.trim_end_matches('.'), left_undone(i + 1)))?;
                let warnings: Vec<&str> = r["warnings"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                if !warnings.is_empty() {
                    emit(format!("exported {path} with warnings: {}", warnings.join("; ")))?;
                } else if print {
                    emit(format!("exported {path}"))?;
                }
                exported.push(path.as_str());
                r
            }
        };
        results.push(r);
    }
    if !print {
        done.push(match total {
            1 => "1 command ran".to_string(),
            n => format!("{n} commands ran"),
        });
        if !exported.is_empty() {
            done.push(format!("exported {}", exported.join(", ")));
        }
        let hint = if shown_answers > 0 { "; --print shows each command's answer" } else { "" };
        emit(format!("OK: {}{hint}", done.join(", ")))?;
    }
    Ok(())
}

fn bench(args: &[String]) -> Result<(), String> {
    let mut file = None;
    let (mut w, mut h, mut iters) = (2880u32, 1800u32, 5u32);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--size" => {
                let v = it.next().ok_or("--size needs WxH")?;
                let (a, b) = v.split_once('x').ok_or("--size needs WxH")?;
                w = a.parse().map_err(|_| "bad width")?;
                h = b.parse().map_err(|_| "bad height")?;
            }
            "--iters" => iters = it.next().and_then(|v| v.parse().ok()).ok_or("--iters needs a number")?,
            f if file.is_none() => file = Some(f.to_string()),
            other => return Err(format!("bench was given `{other}`, which it doesn't take. bench takes FILE, --size WxH and --iters N")),
        }
    }
    let file = file.ok_or("bench takes FILE, the document to render, e.g. bench poster.ai")?;
    let mut hl = Headless::new();
    hl.call("app.open", json!({ "path": file })).map_err(|e| format!("open {file}: {e}"))?;
    let doc = hl.session.doc().map_err(|e| e.to_string())?.doc.clone();
    let b = doc.artboards.first().map(|a| a.rect).ok_or("document has no artboard")?;
    let z = (w as f64 / b.width()).min(h as f64 / b.height()) * 0.95;
    let view = vectorcraft_geom::Affine::translate((w as f64 / 2.0, h as f64 / 2.0))
        * vectorcraft_geom::Affine::scale(z)
        * vectorcraft_geom::Affine::translate(-b.center().to_vec2());
    let opts = vectorcraft_render::RenderOptions::default();
    outln!("{file}: {} nodes, {w}x{h}", doc.layers.iter().map(|l| l.count()).sum::<usize>());
    for threads in [vectorcraft_render::default_threads(), 0] {
        let mut r = vectorcraft_render::Renderer::new();
        r.threads = threads;
        r.render(&doc, w, h, view, &opts);
        let t = std::time::Instant::now();
        for _ in 0..iters {
            r.render(&doc, w, h, view, &opts);
        }
        outln!(
            "  threads {threads}: {:.1} ms/frame (drawn {}, culled {})",
            t.elapsed().as_secs_f64() * 1000.0 / iters as f64,
            r.stats.drawn,
            r.stats.culled
        );
    }
    Ok(())
}

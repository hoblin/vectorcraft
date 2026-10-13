// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_vectorcraft-cli");

fn tmp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("vectorcraft-cli-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// What the CLI printed on stdout (exit 0) or on stderr (exit 1), as one string.
fn stdout(args: &[&str]) -> String {
    let out = Command::new(BIN).args(args).output().unwrap();
    assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}
fn stderr(args: &[&str]) -> String {
    let out = Command::new(BIN).args(args).output().unwrap();
    assert!(!out.status.success(), "{args:?} succeeded: {}", String::from_utf8_lossy(&out.stdout));
    String::from_utf8(out.stderr).unwrap().trim_end().to_string()
}
/// `s` with its whitespace collapsed, so wrapping doesn't matter.
fn words(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn commands_lists_one_command_per_line() {
    let out = stdout(&["commands"]);
    assert!(out.contains("\nshape.rectangle — Rectangle: {x, y, width, height, radius?, rotation?} → {id}\n"), "{out}");
    assert!(out.contains("\nobject.group — Group → {id}\n") && out.contains("\nfile.export — Export…: {"), "{out}");
    assert!(out.contains("\nselect.none — Deselect\n"), "{out}");
    // Grouped by the first segment: a blank line between `select.*` and `shape.*`.
    assert!(out.contains("\n\nshape.arc — Arc"), "{out}");
    let out = stdout(&["commands", "SHAPE."]);
    assert!(out.lines().filter(|l| !l.is_empty()).all(|l| l.to_ascii_lowercase().contains("shape.")), "{out}");
    assert_eq!(stderr(&["commands", "zzzz"]), "vectorcraft-cli: no command's id or name contains `zzzz`; `vectorcraft-cli commands` lists them all");
}

#[test]
fn describe_spells_a_command_out() {
    assert_eq!(
        stdout(&["describe", "shape.rectangle"]),
        "shape.rectangle — Rectangle\n\nParameters: {x, y, width, height, radius?: pt, rotation?: deg (about centre)}\nAnswers: {id}\n\nRun it with: vectorcraft-cli run --cmd shape.rectangle --params '{…}'\n"
    );
    assert_eq!(
        stdout(&["describe", "object.group"]),
        "object.group — Group, in the Object menu\n\nTakes no parameters.\nAnswers: {id}\nActs on the selected objects.\n\nRun it with: vectorcraft-cli run --cmd object.group\n"
    );
    let fill = stdout(&["describe", "paint.setFill"]);
    assert!(
        fill.starts_with("paint.setFill — Fill\n\nParameters: {color?:") && fill.contains("\nActs on `ids`, else on the selected objects.\n"),
        "{fill}"
    );
    assert!(stdout(&["describe", "file.open"]).starts_with("file.open — Open…\n\nParameters: {path}"));
    assert_eq!(stderr(&["describe", "nope.nothing"]), "vectorcraft-cli: unknown command `nope.nothing`. `vectorcraft-cli commands` lists them.");
}

#[test]
fn run_batch_exports() {
    let svg = tmp("batch.svg");
    let png = tmp("batch.png");
    let dc = tmp("batch.vectorcraft");
    let out = Command::new(BIN)
        .args(["run", "--cmd", "file.new", "--params", r#"{"width":200,"height":100}"#])
        .args(["--cmd", "shape.ellipse", "--params", r#"{"x":10,"y":10,"width":80,"height":60}"#])
        .args(["--cmd", "paint.setFill", "--params", r##"{"color":"#3366ff"}"##])
        .args(["--export", svg.to_str().unwrap(), "--export", png.to_str().unwrap(), "--export", dc.to_str().unwrap(), "--scale", "2"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("OK: 3 commands ran, exported {}, {}, {}; --print shows each command's answer\n", svg.display(), png.display(), dc.display())
    );
    assert!(std::fs::read_to_string(&svg).unwrap().contains("3366ff"));
    let png_bytes = std::fs::read(&png).unwrap();
    assert_eq!(&png_bytes[..4], b"\x89PNG");
    // 200×100 pt at scale 2 → 400×200 px (IHDR width/height).
    assert_eq!(u32::from_be_bytes(png_bytes[16..20].try_into().unwrap()), 400);

    // Re-open the native file and inspect.
    let out = stdout(&["run", "--in", dc.to_str().unwrap(), "--print", "--cmd", "document.inspect"]);
    let (open, inspect) = out.trim_end().split_once('\n').unwrap();
    assert_eq!(open, format!("opened {}", dc.display()));
    let inspected: Value = serde_json::from_str(inspect.strip_prefix("document.inspect → ").unwrap()).unwrap();
    assert_eq!(inspected["artboards"][0]["width"], 200.0);
}

/// Without --print, run confirms what it did in one line; with it, each command's answer.
#[test]
fn print_shows_what_each_command_did() {
    let rect = r#"{"x":10,"y":10,"width":50,"height":40}"#;
    assert_eq!(
        stdout(&["run", "--cmd", "shape.rectangle", "--params", rect, "--cmd", "select.none"]),
        "OK: 2 commands ran; --print shows each command's answer\n"
    );
    assert_eq!(stdout(&["run", "--cmd", "select.none"]), "OK: 1 command ran\n");
    assert_eq!(
        stdout(&[
            "run",
            "--print",
            "--cmd",
            "shape.rectangle",
            "--params",
            rect,
            "--cmd",
            "paint.setFill",
            "--params",
            r##"{"color":"#ff0000"}"##,
            "--cmd",
            "select.none"
        ]),
        "shape.rectangle → {\"id\":2}\npaint.setFill → changed the fill of rectangle #2\nselect.none → no object is selected\n"
    );
}

/// A mistake in the flags means nothing ran; the error mirrors what was read and says what to
/// type instead.
#[test]
fn a_flag_mistake_is_read_back_and_nothing_runs() {
    let rect = r#"{"x":10,"y":10,"width":50,"height":40}"#;
    assert_eq!(
        stderr(&["run", "--cmd", "shape.rectangel"]),
        "vectorcraft-cli: --cmd 1 of 1 was read as command `shape.rectangel`, which is not a command. Commands named like it: shape.rectangle, shape.rectangularGrid, path.rectCut; `vectorcraft-cli commands` lists them all. Nothing ran."
    );
    assert_eq!(
        stderr(&["run", "--cmd", r#"{"x":1}"#]),
        "vectorcraft-cli: --cmd 1 of 1 was read as command `{\"x\":1}`, which is not a command. --cmd takes a command id; its parameters go in --params after it. Nothing ran."
    );
    assert_eq!(
        stderr(&["run", "--cmd", "shape.rectangle", "--params", r#"{"x":10,"#]),
        "vectorcraft-cli: --cmd 1 of 1 (`shape.rectangle`) was given --params `{\"x\":10,`, which is not JSON: EOF while parsing a value at line 1 column 8. `shape.rectangle` takes {x, y, width, height, radius?, rotation?}, as one JSON object. Nothing ran."
    );
    assert_eq!(
        stderr(&["run", "--cmd", "shape.rectangle", "--params", "[1]"]),
        "vectorcraft-cli: --cmd 1 of 1 (`shape.rectangle`) was given --params `[1]`, which is not a JSON object. `shape.rectangle` takes {x, y, width, height, radius?, rotation?}, as one JSON object. Nothing ran."
    );
    assert_eq!(
        stderr(&["run", "--params", "{}"]),
        "vectorcraft-cli: --params `{}` follows no --cmd: --params gives the parameters of the --cmd right before it. Nothing ran."
    );
    assert_eq!(stderr(&["run", "--cmd"]), "vectorcraft-cli: --cmd needs a command id, e.g. --cmd shape.rectangle. Nothing ran.");
    assert_eq!(
        stderr(&["run", "--foo"]),
        "vectorcraft-cli: run was given `--foo`, which it doesn't take. run takes --in FILE, --cmd COMMAND, --params JSON (right after its --cmd), --export OUT, --scale N and --print. Nothing ran."
    );
    // A command name mistaken in the third step: nothing ran, the first two included.
    let svg = tmp("nothing-ran.svg");
    let _ = std::fs::remove_file(&svg);
    let err = stderr(&["run", "--cmd", "shape.rectangle", "--params", rect, "--export", svg.to_str().unwrap(), "--cmd", "nope"]);
    assert!(
        err.starts_with("vectorcraft-cli: --cmd 2 of 2 was read as command `nope`, which is not a command.") && err.ends_with(" Nothing ran."),
        "{err}"
    );
    assert!(!svg.exists(), "an export before the mistaken step still didn't run");
}

/// A failing command names its place, what went wrong and what to do, and what after it is left
/// undone.
#[test]
fn a_failing_command_says_which_and_what_is_left_undone() {
    let rect = r#"{"x":10,"y":10,"width":50,"height":40}"#;
    let svg = tmp("undone.svg");
    assert_eq!(
        stderr(&[
            "run",
            "--cmd",
            "shape.rectangle",
            "--params",
            rect,
            "--cmd",
            "object.lock",
            "--params",
            r#"{"ids":[2]}"#,
            "--cmd",
            "select.all",
            "--export",
            svg.to_str().unwrap()
        ]),
        format!(
            "vectorcraft-cli: --cmd 2 of 3 (`object.lock`): invalid parameters for `object.lock`: unknown parameter `ids`; `object.lock` takes no parameters. Nothing after it ran: {} not written.",
            svg.display()
        )
    );
    assert_eq!(
        stderr(&["run", "--cmd", "object.group"]),
        "vectorcraft-cli: --cmd 1 of 1 (`object.group`): command `object.group` is not available right now: nothing selected. It acts on the selected objects; select.set {\"ids\": […]} selects objects, select.all every object."
    );
    assert_eq!(
        stderr(&["run", "--cmd", "select.none", "--cmd", "edit.clear", "--params", r#"{"ids":[999]}"#]),
        "vectorcraft-cli: --cmd 2 of 2 (`edit.clear`): invalid parameters for `edit.clear`: no object #999 in the document; document.inspect lists the objects with their ids; `edit.clear` takes {ids?}."
    );
    assert_eq!(
        stderr(&["run", "--cmd", "artboard.delete", "--params", r#"{"index":5}"#]),
        "vectorcraft-cli: --cmd 1 of 1 (`artboard.delete`): no artboard 5 (0-based; the document has 1)."
    );
    assert!(stderr(&["run", "--in", "nothere.svg", "--cmd", "select.all"]).ends_with("No such file or directory (os error 2). Nothing ran."));
}

/// Every subcommand answers `--help` (and `help SUB`) with its page and does nothing else.
#[test]
fn every_subcommand_answers_help_and_does_nothing_else() {
    let dir = tmp("help-does-nothing");
    std::fs::create_dir_all(&dir).unwrap();
    for sub in ["run", "commands", "describe", "convert", "info", "mcp", "bench", "perf"] {
        let out = Command::new(BIN).args([sub, "--help"]).current_dir(&dir).output().unwrap();
        let page = String::from_utf8(out.stdout).unwrap();
        assert!(out.status.success() && page.starts_with(&format!("usage: vectorcraft-cli {sub}")), "{sub}: {page}");
        assert_eq!(page, stdout(&["help", sub]), "{sub}");
        assert_eq!(page, stdout(&[sub, "-h"]), "{sub}");
    }
    assert!(std::fs::read_dir(&dir).unwrap().next().is_none(), "a --help wrote a file");
    let overview = stdout(&[]);
    assert_eq!(overview, stdout(&["help"]));
    assert_eq!(overview, stdout(&["--help"]));
    assert!(
        overview.starts_with("vectorcraft-cli works on VectorCraft documents")
            && overview.contains("\n  run        run commands on a document, then export it\n"),
        "{overview}"
    );
    assert!(stderr(&["bogus"]).starts_with("vectorcraft-cli: `bogus` is not a subcommand.\n\nvectorcraft-cli works on"));
}

#[test]
fn run_help_says_what_the_ids_do_not() {
    let page = words(&stdout(&["run", "--help"]));
    for fact in [
        "Positions and sizes are in points, x to the right and y down",
        "Every object has an id. document.inspect lists them",
        "A command that draws (shape.*, text.create, file.place…) selects what it drew and answers its id.",
        "Artboard indices start at 0",
        "\"$N.id\" is the id step N answered",
        "Without --print, run ends with one line, e.g. `OK: 3 commands ran, exported logo.svg`",
    ] {
        assert!(page.contains(fact), "{fact}\n{page}");
    }
}

#[test]
fn mcp_headless_over_stdio() {
    let mut child = Command::new(BIN).args(["mcp", "--headless"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        for m in [
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"draw_shape","arguments":{"shape":"polygon","cx":100,"cy":100,"radius":50,"sides":5,"fill":"#00aa00"}}}),
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"screenshot","arguments":{"scale":0.25}}}),
        ] {
            writeln!(stdin, "{m}").unwrap();
        }
    }
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let replies: Vec<Value> = String::from_utf8(out.stdout).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(replies.iter().map(|r| r["id"].as_u64().unwrap()).collect::<Vec<_>>(), [1, 2, 3, 4]);
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "vectorcraft");
    assert_eq!(replies[2]["result"]["isError"], false);
    assert_eq!(replies[3]["result"]["content"][0]["type"], "image");
}

/// `mcp --automation-read-root` / `--automation-write-root` (#832): the flags are checked before the
/// server starts, and the process then reads and writes only inside its roots.
#[test]
fn mcp_automation_roots() {
    let base = tmp("roots");
    for d in ["in", "out", "outside"] {
        std::fs::create_dir_all(base.join(d)).unwrap();
    }
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"><rect width="20" height="10"/></svg>"#;
    std::fs::write(base.join("in/a.svg"), svg).unwrap();
    std::fs::write(base.join("outside/b.svg"), svg).unwrap();
    std::fs::write(base.join("outside/victim.png"), b"keep this file").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(base.join("outside/b.svg"), base.join("in/escape.svg")).unwrap();
        symlink(base.join("outside"), base.join("out/escape")).unwrap();
        symlink(base.join("outside/victim.png"), base.join("out/victim.png")).unwrap();
        symlink(base.join("outside/missing.png"), base.join("out/dangling.png")).unwrap();
    }
    let dir = |d: &str| base.join(d).to_string_lossy().into_owned();
    let fails = |args: &[&str], why: &str| {
        let out = Command::new(BIN).args(args).stdin(Stdio::null()).output().unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success() && err.contains(why), "{args:?}: {err}");
    };
    fails(&["mcp", "--automation-read-root", &dir("missing")], "cannot open the automation read root");
    fails(&["mcp", "--automation-read-root", &dir("in"), "--automation-read-root", &dir("out")], "given twice");
    fails(&["mcp", "--automation-write-root"], "needs a folder");
    fails(&["mcp", "--connect", "127.0.0.1:1", &format!("--automation-write-root={}", dir("out"))], "a running app enforces its own roots");

    let mut child = Command::new(BIN)
        .args(["mcp", "--automation-read-root", &dir("in"), &format!("--automation-write-root={}", dir("out"))])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let call = |id: u64, name: &str, args: Value| json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}});
    let file = |d: &str, f: &str| base.join(d).join(f).to_string_lossy().into_owned();
    let mut calls = vec![
        call(1, "open_file", json!({"path": file("in", "a.svg")})),
        call(2, "open_file", json!({"path": file("outside", "b.svg")})),
        call(3, "export", json!({"path": file("out", "a.png")})),
        call(4, "export", json!({"path": file("in", "a.png")})),
        call(5, "save_file", json!({"path": file("outside", "a.vectorcraft")})),
        call(6, "open_file", json!({"path": file("in", "../outside/b.svg")})),
        call(7, "export", json!({"path": file("out", "../outside/traversal.png")})),
    ];
    let mut expected_errors = vec![false, true, false, true, true, true, true];
    #[cfg(unix)]
    {
        calls.extend([
            call(8, "open_file", json!({"path": file("in", "escape.svg")})),
            call(9, "export", json!({"path": file("out", "escape/escaped.png")})),
            call(10, "export", json!({"path": file("out", "victim.png")})),
            call(11, "export", json!({"path": file("out", "dangling.png")})),
        ]);
        expected_errors.extend([true; 4]);
    }
    // An access error is per request: the server continues and can still save inside its root.
    calls.push(call(12, "save_file", json!({"path": file("out", "allowed.vectorcraft")})));
    expected_errors.push(false);
    {
        let mut stdin = child.stdin.take().unwrap();
        for m in calls {
            writeln!(stdin, "{m}").unwrap();
        }
    }
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let replies: Vec<Value> = String::from_utf8(out.stdout).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let errors: Vec<bool> = replies.iter().map(|r| r["result"]["isError"] == true).collect();
    assert_eq!(errors, expected_errors, "{replies:?}");
    assert!(replies[1]["result"]["content"][0]["text"].as_str().unwrap().contains("outside the read root"));
    assert!(base.join("out/a.png").is_file() && !base.join("in/a.png").exists() && !base.join("outside/a.vectorcraft").exists());
    assert!(base.join("out/allowed.vectorcraft").is_file());
    for denied in ["traversal.png", "escaped.png", "missing.png"] {
        assert!(!base.join("outside").join(denied).exists(), "{denied}");
    }
    assert_eq!(std::fs::read(base.join("outside/victim.png")).unwrap(), b"keep this file");
}

/// End-to-end protocol sweep over stdio against the compiled binary. The in-process
/// tests drive `Server::handle_line` directly, so the binary's own wiring (argument
/// parsing, logger install, stdout pollution, notification order) is untested: this
/// drives the real `mcp --headless` process through prompts, completions, resource
/// templates, logging and the error taxonomy instead.
#[test]
fn mcp_protocol_over_stdio() {
    use std::io::{BufRead, BufReader};
    let mut child =
        Command::new(BIN).args(["mcp", "--headless"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut notes = vec![];

    fn send(stdin: &mut std::process::ChildStdin, v: Value) {
        writeln!(stdin, "{v}").unwrap();
    }
    // Next reply with this id; notifications (no id) are stashed for later.
    fn recv(lines: &mut std::io::Lines<BufReader<std::process::ChildStdout>>, notes: &mut Vec<Value>, id: u64) -> Value {
        loop {
            let line = lines.next().unwrap().unwrap();
            let v: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(v["jsonrpc"], "2.0", "{v}");
            if v.get("id").and_then(|i| i.as_u64()) == Some(id) {
                return v;
            }
            notes.push(v);
        }
    }

    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}),
    );
    let r = recv(&mut lines, &mut notes, 1);
    assert_eq!(r["result"]["serverInfo"]["name"], "vectorcraft");
    assert!(r["result"]["capabilities"]["prompts"].is_object(), "{r}");
    send(&mut stdin, json!({"jsonrpc":"2.0","method":"notifications/initialized"}));

    send(&mut stdin, json!({"jsonrpc":"2.0","id":2,"method":"prompts/list","params":{}}));
    let r = recv(&mut lines, &mut notes, 2);
    assert!(r["result"]["prompts"].as_array().is_some_and(|p| !p.is_empty()), "{r}");
    send(&mut stdin, json!({"jsonrpc":"2.0","id":3,"method":"prompts/get","params":{"name":"poster","arguments":{"brief":"a jazz festival"}}}));
    let r = recv(&mut lines, &mut notes, 3);
    assert!(r["result"]["messages"][0]["content"]["text"].as_str().is_some_and(|t| t.contains("a jazz festival")), "{r}");

    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":4,"method":"completion/complete","params":{"ref":{"type":"ref/prompt","name":"export-set"},"argument":{"name":"formats","value":"sv"}}}),
    );
    let r = recv(&mut lines, &mut notes, 4);
    assert!(r["result"]["completion"]["values"].as_array().is_some_and(|v| v.contains(&json!("svg"))), "{r}");

    send(&mut stdin, json!({"jsonrpc":"2.0","id":5,"method":"resources/list","params":{}}));
    assert!(recv(&mut lines, &mut notes, 5)["result"]["resources"].as_array().is_some_and(|v| v.len() == 3));
    send(&mut stdin, json!({"jsonrpc":"2.0","id":6,"method":"resources/templates/list","params":{}}));
    assert!(recv(&mut lines, &mut notes, 6)["result"]["resourceTemplates"].as_array().is_some_and(|v| v.len() >= 4));

    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"draw_shape","arguments":{"shape":"rectangle","x":10,"y":10,"width":60,"height":40}}}),
    );
    let r = recv(&mut lines, &mut notes, 7);
    assert_eq!(r["result"]["isError"], false, "{r}");
    let id: i64 = serde_json::from_str::<Value>(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap()["id"].as_i64().unwrap();

    send(&mut stdin, json!({"jsonrpc":"2.0","id":8,"method":"resources/read","params":{"uri": format!("vectorcraft://object/{id}")}}));
    let r = recv(&mut lines, &mut notes, 8);
    let body: Value = serde_json::from_str(r["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["id"].as_i64(), Some(id), "{body}");

    // Unknown resources (-32002) and bad template values (-32602) stay apart.
    send(&mut stdin, json!({"jsonrpc":"2.0","id":9,"method":"resources/read","params":{"uri":"vectorcraft://nope" }}));
    assert_eq!(recv(&mut lines, &mut notes, 9)["error"]["code"], -32002);
    send(&mut stdin, json!({"jsonrpc":"2.0","id":10,"method":"resources/read","params":{"uri":"vectorcraft://object/"}}));
    assert_eq!(recv(&mut lines, &mut notes, 10)["error"]["code"], -32602);

    // Large containers slice through run_command document.node: a truncated level reports
    // childCount, and a bad limit is a tool error, not a crash or a full read.
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":20,"method":"tools/call","params":{"name":"draw_shape","arguments":{"shape":"ellipse","x":90,"y":10,"width":30,"height":30}}}),
    );
    assert_eq!(recv(&mut lines, &mut notes, 20)["result"]["isError"], false);
    let run = |id: u64, command: &str, params: Value| json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"run_command","arguments":{"command":command,"params":params}}});
    send(&mut stdin, run(21, "select.all", json!({})));
    recv(&mut lines, &mut notes, 21);
    send(&mut stdin, run(22, "object.group", json!({})));
    let r = recv(&mut lines, &mut notes, 22);
    let group = serde_json::from_str::<Value>(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap()["id"].clone();
    send(&mut stdin, run(23, "document.node", json!({"id": group, "summary": true, "childLimit": 1})));
    let r = recv(&mut lines, &mut notes, 23);
    let sliced: Value = serde_json::from_str(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(sliced["children"].as_array().map(Vec::len), Some(1), "{sliced}");
    assert_eq!(sliced["childCount"], 2, "{sliced}");
    send(&mut stdin, run(24, "document.node", json!({"id": group, "summary": true, "depth": -1})));
    assert_eq!(recv(&mut lines, &mut notes, 24)["result"]["isError"], true);

    // Skeleton then drill: a depth-0 inspect is the layer skeleton with counts, and
    // document.find locates nodes without dumping the tree.
    send(&mut stdin, run(25, "document.inspect", json!({"depth": 0})));
    let r = recv(&mut lines, &mut notes, 25);
    let skel: Value = serde_json::from_str(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(skel["layers"][0].get("children").is_none(), "{skel}");
    assert!(skel["artboards"].as_array().is_some_and(|a| !a.is_empty()), "{skel}");
    send(&mut stdin, run(26, "document.find", json!({"kind": "ellipse"})));
    let r = recv(&mut lines, &mut notes, 26);
    let found: Value = serde_json::from_str(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(found["total"], 1, "{found}");
    assert!(found["matches"][0]["path"].as_array().is_some_and(|p| !p.is_empty()), "{found}");
    send(&mut stdin, run(27, "document.find", json!({})));
    assert_eq!(recv(&mut lines, &mut notes, 27)["result"]["isError"], true);

    // The binary installed its logger: records arrive as notifications/message, and only
    // after the client asked for them.
    send(&mut stdin, json!({"jsonrpc":"2.0","id":11,"method":"logging/setLevel","params":{"level":"debug"}}));
    assert!(recv(&mut lines, &mut notes, 11).get("error").is_none());
    send(&mut stdin, json!({"jsonrpc":"2.0","id":12,"method":"tools/list","params":{}}));
    assert!(recv(&mut lines, &mut notes, 12)["result"]["tools"].as_array().is_some_and(|v| !v.is_empty()));

    // Malformed input is a parse error, and the server keeps serving afterwards.
    stdin.write_all(b"{not json\n").unwrap();
    let parse_err: Value = loop {
        let line = lines.next().unwrap().unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        if v.get("error").is_some() {
            break v;
        }
        notes.push(v);
    };
    assert_eq!(parse_err["error"]["code"], -32700);
    assert!(parse_err["id"].is_null(), "{parse_err}");
    send(&mut stdin, json!({"jsonrpc":"2.0","id":13,"method":"ping","params":{}}));
    assert!(recv(&mut lines, &mut notes, 13).get("error").is_none());
    drop(stdin);
    // Drain whatever the debug window queued; every line off the protocol stream parses.
    for line in lines.by_ref() {
        let line = line.unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["jsonrpc"], "2.0", "{v}");
        notes.push(v);
    }
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("panicked"));
    let messages = notes.iter().filter(|n| n["method"] == "notifications/message").count();
    assert!(messages > 0, "no log records after logging/setLevel: {notes:?}");
}

/// `vectorcraft-cli commands | head -1` panicked with "failed printing to
/// stdout: Broken pipe (os error 32)" and exit status 101.
#[test]
fn closed_stdout_ends_quietly() {
    use std::io::Read;
    let mut child = Command::new(BIN).arg("commands").stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut first = [0u8; 16];
    child.stdout.as_mut().unwrap().read_exact(&mut first).unwrap();
    drop(child.stdout.take());
    let out = child.wait_with_output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success() && !err.contains("panicked"), "{:?}: {err}", out.status);
}

/// #1005: `run` whose reader has gone (`vectorcraft-cli run … | true`) ends quietly with status 0,
/// and its later steps still run; a failing step still fails.
#[test]
fn run_with_a_closed_stdout_ends_quietly_and_finishes_the_batch() {
    let svg = tmp("closed-run.svg");
    let _ = std::fs::remove_file(&svg);
    let rect = r#"{"x":1,"y":1,"width":5,"height":5}"#;
    let args = ["run", "--cmd", "document.json", "--cmd", "shape.rectangle", "--params", rect, "--export", svg.to_str().unwrap()];
    let mut child = Command::new(BIN).args(args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    drop(child.stdout.take());
    let out = child.wait_with_output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success() && err.is_empty(), "{:?}: {err}", out.status);
    assert!(svg.is_file(), "the export after the closed reader still ran");
    let out = Command::new(BIN).args(["run", "--cmd", "not-a-command"]).stdout(Stdio::null()).output().unwrap();
    assert!(!out.status.success());
}

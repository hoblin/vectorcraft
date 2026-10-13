//! The help pages: what an agent reads before it types a command, so they say what the ids and
//! flags don't.

pub struct Subcommand {
    pub name: &'static str,
    pub usage: &'static str,
    pub summary: &'static str,
    pub details: &'static str,
}

pub const SUBCOMMANDS: &[Subcommand] = &[
    Subcommand {
        name: "run",
        usage: "run [--in FILE] [--cmd COMMAND [--params JSON]]... [--export OUT]... [--scale N] [--print]",
        summary: "run commands on a document, then export it",
        details: "Runs commands, in order, on one document: FILE with --in (any readable format), else a new
612×792 pt (Letter) document with one artboard. Each --export writes the document as it is at that
point, in the format OUT's extension names: .vectorcraft keeps everything; .svg, .pdf, .ai, .png,
.jpg and the others are written as convert writes them, --scale N pixels per point for the raster
ones (default 1).

Each --cmd is one command: COMMAND is a command id, and --params, right after it, its parameters
as one JSON object. Quote it for the shell. A command that needs no parameters is just its id.

  vectorcraft-cli run \\
    --cmd file.new --params '{\"width\":400,\"height\":300}' \\
    --cmd shape.rectangle --params '{\"x\":20,\"y\":20,\"width\":360,\"height\":260,\"radius\":24}' \\
    --cmd paint.setFill --params '{\"color\":\"#0047bb\"}' \\
    --cmd text.create --params '{\"x\":40,\"y\":190,\"text\":\"bondata\",\"size\":72,\"color\":\"#ffffff\"}' \\
    --export logo.svg --export logo.png --scale 2

`vectorcraft-cli commands` lists the command ids; `vectorcraft-cli describe COMMAND` explains one
and its parameters.

What the ids don't say:
- Positions and sizes are in points, x to the right and y down from the document's origin, the
  top-left corner of the first artboard. Angles are degrees. A colour is \"#rrggbb\", [r, g, b] in
  0..1 or {c, m, y, k}; a swatch goes by name in `swatch` where a command takes it.
- Every object has an id. document.inspect lists them with each object's kind, name and bounds.
  An id is looked up in the whole active document, whatever artboard or layer the object is on.
- A command that draws (shape.*, text.create, file.place…) selects what it drew and answers its id.
  paint.*, stroke.*, object.*, effect.* and text.set* act on the selected objects, or on the
  objects named in `ids` where the command takes them (describe says); select.set {\"ids\": […]}
  selects others, select.all everything, select.none nothing.
- Artboard indices start at 0 (artboard.*, document.export {\"artboard\": N}); convert's --range
  counts from 1.
- A later step can use an earlier one's answer: \"$N.id\" is the id step N answered (steps count
  from 0; --in's open is not a step) and \"$last.id\" the step before. \"$$\" stands for one \"$\".

If a command fails, run stops there: nothing after it runs or is exported. Without --print, run
ends with one line, e.g. `OK: 3 commands ran, exported logo.svg`; --print shows each command's
answer instead, such as the id of a drawn shape or what a command changed.",
    },
    Subcommand {
        name: "commands",
        usage: "commands [FILTER]",
        summary: "list the commands run can use, one per line",
        details: "Lists the commands that run --cmd and the MCP server take, one per line, grouped by their
first segment: the id, its name in the app and its parameters. FILTER keeps the commands whose id
or name contains it, ignoring case.

  vectorcraft-cli commands shape.

In the parameters, `?` marks an optional one, a `|` separates alternatives and `→` shows what the
command answers. `vectorcraft-cli describe COMMAND` spells one command out.",
    },
    Subcommand {
        name: "describe",
        usage: "describe COMMAND",
        summary: "explain one command and its parameters",
        details: "Explains one command: what it is in the app, its parameters in full, what it answers, what it
acts on, and how run calls it.

  vectorcraft-cli describe shape.rectangle",
    },
    Subcommand {
        name: "convert",
        usage: "convert IN OUT [--scale N] [--artboard I | --range R] [--outline-text] [--decimals N] [--replace-lossy] [--editing-data]",
        summary: "save a document in the format OUT's extension names",
        details: "Opens IN (any readable format) and writes it to OUT in the format OUT's extension names: a PDF
gets every artboard unless one is chosen, EPS the bounds of the art, the other formats the first
artboard. Live effects are kept, and so are hidden layers and objects (written hidden in SVG and
PSD) and SVG's data-* attributes. Prints what was written as JSON.

  --scale N        pixels per point for a raster OUT, 1 by default
  --artboard I     the artboard at index I; indices start at 0
  --range R        artboards counted from 1, such as \"1-3,5\"
  --outline-text   write SVG text as paths
  --decimals N     SVG coordinate precision, 1–7, 3 by default
  --replace-lossy  overwrite IN with OUT even when reading IN left things out (hidden text, art or
                   layers it could not read; the import notes say which)
  --editing-data   open a .ai from its editing data even where that draws differently from its
                   PDF part (which can be older), as long as it can be read

  vectorcraft-cli convert logo.ai logo.svg",
    },
    Subcommand {
        name: "info",
        usage: "info FILE",
        summary: "print a document's artboards, objects and fonts as JSON",
        details: "Prints FILE as JSON: the import warnings (what didn't come in as it was, such as an EPS read
from its preview, and why), title, colour mode, units, artboards, object counts by kind and fonts.

The objects themselves, with their ids, come from the document.inspect command:

  vectorcraft-cli run --in logo.svg --print --cmd document.inspect",
    },
    Subcommand {
        name: "mcp",
        usage: "mcp [--connect ADDR | --headless] [--automation-read-root DIR] [--automation-write-root DIR]",
        summary: "serve the commands to an agent over MCP on stdio",
        details: "Runs a Model Context Protocol server on stdin and stdout. By default it connects to the
VectorCraft app started with `vectorcraft --control 7979` at 127.0.0.1:7979, and works on its own
document without a window when no app answers.

  --connect ADDR              the app's control address; fails when no app answers there
  --headless                  its own document without a window, without looking for the app
  --automation-read-root DIR  confine the files the agent's commands read (open, place, relink,
                              library loads…) to DIR, links followed
  --automation-write-root DIR confine the files they write (save, export, package, library
                              saves…) to DIR, links followed

A root left out grants none of its access. The roots imply --headless: a running app enforces its
own (vectorcraft --control PORT --automation-read-root DIR …).",
    },
    Subcommand {
        name: "bench",
        usage: "bench FILE [--size WxH] [--iters N]",
        summary: "time rendering a file",
        details: "Renders FILE (any readable format) fitted to WxH pixels (2880x1800 by default) N times (5 by
default) and prints the milliseconds per frame, multithreaded and single-threaded.

  vectorcraft-cli bench poster.ai --size 1920x1080",
    },
    Subcommand {
        name: "perf",
        usage: "perf [--paths N] [--deep]",
        summary: "check the performance budgets",
        details: "Checks the performance budgets (render, pan, hit test, save and load, SVG, Pathfinder, handle
and segment drags) on a synthetic document of N paths (50000 by default). Exits non-zero when a
budget is exceeded, unless the report says the machine is busy.

  --deep   also time work with no budget yet and print memory use (the working set on Windows)",
    },
    Subcommand { name: "version", usage: "--version", summary: "print the version", details: "Prints the version of vectorcraft-cli." },
];

/// The page for `name`: a subcommand's, or the overview for `help`, `--help` and `-h`.
pub fn page(name: &str) -> Option<String> {
    let name = match name {
        "help" | "--help" | "-h" => return Some(overview()),
        "--version" | "-V" => "version",
        name => name,
    };
    SUBCOMMANDS.iter().find(|s| s.name == name).map(|s| format!("usage: vectorcraft-cli {}\n\n{}\n", s.usage, s.details))
}

/// One line per subcommand, the one example, and the formats run, convert and info read and write.
pub fn overview() -> String {
    use vectorcraft_engine::cmd::fileio::{OPEN_EXTS, export_extensions};
    let width = SUBCOMMANDS.iter().map(|s| first_word(s.usage).len()).max().unwrap_or_default();
    let list: Vec<String> = SUBCOMMANDS.iter().map(|s| format!("  {:<width$}  {}", first_word(s.usage), s.summary)).collect();
    format!(
        "vectorcraft-cli works on VectorCraft documents (.vectorcraft, .svg, .ai, .pdf…) without a window.

{}

A document is drawn by running commands on it:

  vectorcraft-cli run --cmd shape.rectangle --params '{{\"x\":10,\"y\":10,\"width\":100,\"height\":50}}' --export logo.svg

`vectorcraft-cli help SUBCOMMAND`, or `vectorcraft-cli SUBCOMMAND --help`, explains one subcommand;
`vectorcraft-cli help run` explains commands.

Readable formats: .{}
Writable formats: .{}
",
        list.join("\n"),
        OPEN_EXTS.join(", ."),
        export_extensions().join(", .")
    )
}

fn first_word(usage: &str) -> &str {
    usage.split(' ').next().unwrap_or_default()
}

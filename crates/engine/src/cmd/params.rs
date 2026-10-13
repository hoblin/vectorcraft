//! The parameter keys a command accepts, read from its params description (#1442).
//!
//! A description starts with the params object: `{x, width?: pt, name?|title?: …, sides=6, …}`, or
//! with the objects a command takes instead of one another: `{path} or {name, dataBase64}`,
//! `{name} | {names: […]}`. Each entry of an object starts with a key, or with alternatives split
//! by `|`; a `?` marks an optional one, and a `:`, `=` or `(` starts what the entry says about it. An entry that starts
//! with `…` stands for keys listed elsewhere: those of each command it names after `as` or `of`
//! (`…settings as file.new`), or, naming none, any key (the options of the format an export picks). Objects inside a value aren't
//! described key by key, so only the top level is checked.
//!
//! Callers outside the app (the control channel, MCP, the CLI) are answered with a parameter error
//! for a key the description doesn't name ([`check_params`]): a misspelt `widht` no longer runs the
//! command with its default. So are the `params` of an effect (#1592): `effect.apply` and
//! `effect.setParams` answer a key the effect doesn't take, or a value of another type than its
//! default, with a parameter error ([`check_effect_params`]). [`Session::execute`] itself takes
//! what it is given, so the journal of an older version still replays.

use serde_json::Value;

use super::{bad, find_command};
use crate::Result;

/// What a params description says about the keys of the params object.
#[derive(Debug, Default, PartialEq)]
pub struct ParamKeys<'a> {
    /// The keys it names.
    pub keys: Vec<&'a str>,
    /// Its `…` entries, without the `…`.
    pub more: Vec<&'a str>,
}

/// The text between the braces of the object `doc` starts with and what follows it, or why it
/// has none.
fn object_body(doc: &str) -> std::result::Result<(&str, &str), String> {
    // (Named, not written in the `Err`: these are for the tests of the descriptions, not messages
    // the app shows, which the message catalogs would have to translate.)
    let (no_object, not_closed) = ("the description doesn't start with the params object `{…}`", "the params object `{…}` isn't closed");
    if !doc.starts_with('{') {
        return Err(no_object.into());
    }
    let (mut depth, mut quote) = (0usize, None);
    for (i, c) in doc.char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '`') => quote = Some(c),
            (None, '{' | '[' | '(') => depth += 1,
            (None, '}' | ']' | ')') => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Ok((doc.get(1..i).unwrap_or_default(), doc.get(i + 1..).unwrap_or_default()));
                }
            }
            _ => {}
        }
    }
    Err(not_closed.into())
}

/// `s` split at each `sep` that is outside brackets, quotes and backticks.
fn split_top(s: &str, sep: char) -> Vec<&str> {
    let (mut out, mut depth, mut quote, mut start) = (Vec::new(), 0usize, None, 0);
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '`') => quote = Some(c),
            (None, '{' | '[' | '(') => depth += 1,
            (None, '}' | ']' | ')') => depth = depth.saturating_sub(1),
            (None, _) if c == sep && depth == 0 => {
                out.push(s.get(start..i).unwrap_or_default());
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(s.get(start..).unwrap_or_default());
    out
}

fn is_key(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// One entry of a params object: the keys it names (each with whether it is optional), or what a
/// `…` entry stands for.
enum Entry<'a> {
    Keys(Vec<(&'a str, bool)>),
    More(&'a str),
}

/// The entries of the params object(s) `doc` starts with (`{…} or {…}`, `{…} | {…}`: the objects
/// the command takes instead of one another), or what in it doesn't read as one.
fn entries(doc: &str) -> std::result::Result<Vec<Entry<'_>>, String> {
    let (mut body, mut rest) = object_body(doc)?;
    let mut raw = split_top(body, ',');
    while let Some(next) = rest.trim_start().strip_prefix("or ").or_else(|| rest.trim_start().strip_prefix("| ")).filter(|r| r.starts_with('{')) {
        (body, rest) = object_body(next)?;
        raw.extend(split_top(body, ','));
    }
    let mut out = vec![];
    for entry in raw.into_iter().map(str::trim).filter(|e| !e.is_empty()) {
        if let Some(more) = entry.strip_prefix('…') {
            out.push(Entry::More(more.trim()));
            continue;
        }
        // `a?|b?: note` names two keys, and so does `a?: note | b?: note`; in `a?: x|y` the `y`
        // is a value.
        let (mut keys, mut noted) = (vec![], false);
        for alternative in split_top(entry, '|') {
            let head = alternative.split([':', '=', '(']).next().unwrap_or_default();
            let rest = alternative.get(head.len()..).unwrap_or_default();
            let optional = head.trim_end().ends_with('?');
            let name = head.trim().trim_end_matches('?').trim_end();
            if !noted {
                if !is_key(name) {
                    let start: String = entry.chars().take(60).collect();
                    let why = format!("`{start}` doesn't start with a parameter name");
                    return Err(why);
                }
                keys.push((name, optional));
                noted = !rest.is_empty();
            } else if is_key(name) && (optional || rest.starts_with(':')) {
                keys.push((name, optional));
            }
        }
        out.push(Entry::Keys(keys));
    }
    Ok(out)
}

/// The keys a params description names, or what in it doesn't read as one.
pub fn doc_keys(doc: &str) -> std::result::Result<ParamKeys<'_>, String> {
    let mut out = ParamKeys::default();
    for entry in entries(doc)? {
        match entry {
            Entry::Keys(keys) => out.keys.extend(keys.into_iter().map(|(name, _)| name)),
            Entry::More(more) => out.more.push(more),
        }
    }
    Ok(out)
}

/// `doc` split at the `→` outside brackets that starts what the command answers: the parameters
/// and, when it says, the answer.
pub fn parts(doc: &str) -> (&str, Option<&str>) {
    let (mut depth, mut quote) = (0usize, None);
    for (i, c) in doc.char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '`') => quote = Some(c),
            (None, '{' | '[' | '(') => depth += 1,
            (None, '}' | ']' | ')') => depth = depth.saturating_sub(1),
            (None, '→') if depth == 0 => return (doc.get(..i).unwrap_or_default().trim_end(), doc.get(i + c.len_utf8()..).map(str::trim)),
            _ => {}
        }
    }
    (doc, None)
}

/// The params object of `doc` with only its keys, `?` marking the optional ones: `{x, y, width,
/// height, radius?, rotation?}`, `{}` for a command that takes none. What an error names as what
/// the command takes (its full description can run to pages).
pub fn outline(doc: &str) -> String {
    let entries = entries(doc).unwrap_or_default();
    let shown: Vec<String> = entries
        .iter()
        .map(|e| match e {
            Entry::Keys(keys) => {
                keys.iter().map(|(name, optional)| format!("{name}{}", if *optional { "?" } else { "" })).collect::<Vec<_>>().join("|")
            }
            Entry::More(more) => format!("…{more}"),
        })
        .collect();
    format!("{{{}}}", shown.join(", "))
}

/// The commands whose keys a `…` entry stands for: those it names after `as` or `of` (`settings as
/// file.new`, `the PDF options of document.exportPdf`), not one it only points to (`see …`).
fn named_commands(more: &str) -> impl Iterator<Item = &'static super::CommandSpec> + '_ {
    let words: Vec<&str> = more.split_whitespace().collect();
    let id = |w: &str| find_command(w.trim_matches(|c: char| !c.is_ascii_alphanumeric()));
    (1..words.len()).filter_map(move |i| id(words.get(i)?).filter(|_| matches!(words.get(i - 1), Some(&"as" | &"of"))))
}

/// Every key command `doc` accepts, the keys of the commands its `…` entries name included.
/// `None`: any key (a `…` entry naming no command, or a description that doesn't parse, which
/// the tests rule out).
fn accepted(doc: &'static str, depth: usize) -> Option<Vec<&'static str>> {
    let parsed = doc_keys(doc).ok()?;
    let mut keys = parsed.keys;
    for more in parsed.more {
        let mut named = named_commands(more).peekable();
        // A reference that leads back here would never end: descriptions refer one or two deep.
        if named.peek().is_none() || depth > 4 {
            return None;
        }
        for spec in named {
            // A command that isn't checked yet doesn't vouch for the ones that take its keys.
            if UNCHECKED.contains(&spec.id) {
                return None;
            }
            keys.extend(accepted(spec.params, depth + 1)?);
        }
    }
    Some(keys)
}

/// Does the description `doc` name `key` as a parameter (its own keys, and those of the commands
/// its `…` entries name)?
pub fn takes_key(doc: &'static str, key: &str) -> bool {
    doc_keys(doc).is_ok_and(|parsed| parsed.keys.contains(&key)) || accepted(doc, 0).is_some_and(|keys| keys.contains(&key))
}

/// `params` has only keys the description `doc` of command `id` names, or the parameter error
/// naming the first one it doesn't and what the command takes.
pub fn check_keys(id: &str, doc: &'static str, params: &Value) -> Result<()> {
    let Value::Object(given) = params else { return Ok(()) };
    if given.is_empty() {
        return Ok(());
    }
    let Some(keys) = accepted(doc, 0) else { return Ok(()) };
    match given.keys().find(|k| !keys.contains(&k.as_str())) {
        Some(key) => Err(unknown(id, key, takes(id, &outline(doc)))),
        None => Ok(()),
    }
}

/// `` `{id}` takes {outline} ``, or that it takes none. The tail of every parameter error, so the
/// agent that sent them sees the keys without another call.
pub fn takes(id: &str, outline: &str) -> String {
    match outline {
        "{}" => format!("`{id}` takes no parameters"),
        outline => format!("`{id}` takes {outline}"),
    }
}

/// The parameter error of command `id` for `key`, with `takes`, what is taken instead. It doesn't
/// guess which key was meant: the outline shows them, and the agent knows what it wanted.
fn unknown(id: &str, key: &str, takes: String) -> crate::EngineError {
    bad(id, format!("unknown parameter `{key}`; {takes}"))
}

/// `params` of built-in effect `effect`, as command `id` was given them (#1592): only keys the
/// effect takes (those of its dialog defaults and of its params description), each of its
/// default's type. Numbers out of range pass: the effect clamps them when it draws. An effect
/// plug-in resolves its own against its manifest, and an unknown effect is the command's to report.
pub fn check_effect_params(id: &str, effect: &str, params: &Value) -> Result<()> {
    let Value::Object(given) = params else { return Ok(()) };
    if effect.starts_with(vectorcraft_plugins::effect::PREFIX) {
        return Ok(());
    }
    let Some(info) = vectorcraft_render::effects::effect_info(effect) else { return Ok(()) };
    let defaults = info.defaults.as_object();
    let mut keys: Vec<&str> = defaults.into_iter().flat_map(|d| d.keys().map(String::as_str)).collect();
    // A description that doesn't parse names no more keys (the tests rule that out).
    for key in doc_keys(info.params).map(|d| d.keys).unwrap_or_default() {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    for (key, v) in given {
        if !keys.contains(&key.as_str()) {
            return Err(unknown(id, key, takes(effect, &format!("{{{}}}", keys.join(", ")))));
        }
        match defaults.and_then(|d| d.get(key)) {
            Some(Value::Number(_)) if !v.is_number() => return Err(bad(id, format!("`{key}` must be a number"))),
            Some(Value::Bool(_)) if !v.is_boolean() => return Err(bad(id, format!("`{key}` must be true or false"))),
            // (A colour is also taken as `[r, g, b]` or as a colour object.)
            Some(Value::String(_)) if v.is_number() || v.is_boolean() => return Err(bad(id, format!("`{key}` must be a string"))),
            _ => {}
        }
    }
    Ok(())
}

/// Commands whose params aren't checked yet (#1442): they read keys in ways the audit of the
/// descriptions couldn't follow (a list of keys in a loop, a whole object read as settings), so
/// their descriptions aren't proven to name every key they take. Sorted; take a command out once
/// its description is checked against its code.
pub const UNCHECKED: &[&str] = &[
    "appearance.copyFrom",
    "appearance.setItem",
    "artboard.rearrange",
    "assets.export",
    "assets.settings.set",
    "attributes.info",
    "attributes.set",
    "brush.freehand",
    "css.export",
    "css.generate",
    "css.selection",
    "document.dxfInfo",
    "document.exportDxf",
    "document.exportEps",
    "document.exportForScreens",
    "document.exportForWeb",
    "document.exportForWeb.preview",
    "document.exportPdf",
    "document.find",
    "document.inspect",
    "document.node",
    "document.open",
    "document.pdfInfo",
    "document.pdfSettings",
    "document.rasterEffectsSettings",
    "edit.assignProfile",
    "edit.colors.adjustBalance",
    "edit.colors.invert",
    "edit.colors.saturate",
    "edit.colors.toCMYK",
    "edit.colors.toGrayscale",
    "edit.colors.toRGB",
    "edit.paste",
    "edit.pasteInBack",
    "edit.pasteInFront",
    "edit.pasteInPlace",
    "edit.pasteOnAllArtboards",
    "edit.pasteWithoutFormatting",
    "effect.apply",
    "eyedropper.setOptions",
    "file.info",
    "file.newFromTemplate",
    "file.newPresets.save",
    "file.place",
    "file.place.info",
    "file.place.queue",
    "flattener.presets.save",
    "flattener.preview",
    "graph.design",
    "graph.setType",
    "graphicStyle.loadLibrary",
    "layer.new",
    "layer.newSublayer",
    "layer.releaseToLayersBuild",
    "layer.setProps",
    "library.add",
    "library.deleteGroup",
    "library.moveItem",
    "library.removeItem",
    "library.renameGroup",
    "library.use",
    "links.placementOptions",
    "livePaint.fill",
    "livePaint.strokeEdge",
    "magicWand.set",
    "object.align",
    "object.blend.make",
    "object.blend.options",
    "object.distribute",
    "object.distributeSpacing",
    "object.expand",
    "object.flattenTransparency",
    "object.liquify",
    "object.path.splitIntoGrid",
    "object.puppetWarp",
    "object.rasterize",
    "object.repeat.options",
    "object.setBounds",
    "object.setLiveShape",
    "object.setOverprint",
    "object.setProps",
    "object.slice.divide",
    "object.slice.options",
    "object.transformAgain",
    "paint.complement",
    "paint.editGradient",
    "paint.freeform.addLine",
    "paint.freeform.addPoint",
    "paint.freeform.deletePoint",
    "paint.freeform.setPoint",
    "paint.freeform.splitLine",
    "paint.invert",
    "paint.lastColor",
    "paint.lastGradient",
    "paint.setFill",
    "paint.setGradientGeom",
    "path.appendAnchor",
    "pattern.options",
    "pdf.preset.save",
    "perspective.grid.define",
    "perspective.presets.save",
    "print.tiling",
    "recolor.apply",
    "select.editSaved",
    "shape.flare",
    "shape.polarGrid",
    "shape.rectangularGrid",
    "stroke.set",
    "stroke.setAdvanced",
    "swatch.edit",
    "swatch.library.load",
    "text.setFormat",
    "text.setRangeStyle",
    "text.setStyle",
    "text.tabs.clear",
    "text.tabs.get",
    "text.tabs.set",
    "transparency.disableOpacityMask",
    "transparency.enableOpacityMask",
    "transparency.linkOpacityMask",
    "transparency.set",
    "transparency.setOpacityMask",
    "transparency.unlinkOpacityMask",
    "type.pathOptions",
    "webExport.presets.save",
    "webExport.settings",
];

/// [`check_keys`] for an engine command, the steps of a `command.batch` included, and
/// [`check_effect_params`] for the effect `effect.apply` adds. A command the engine doesn't have
/// passes: running it reports that.
pub fn check_params(id: &str, params: &Value) -> Result<()> {
    let Some(spec) = find_command(id) else { return Ok(()) };
    if let ("effect.apply" | "appearance.addEffect", Some(effect), Some(given)) =
        (id, params.get("effect").or_else(|| params.get("id")).and_then(Value::as_str), params.get("params"))
    {
        check_effect_params(id, effect, given)?;
    }
    if UNCHECKED.contains(&id) {
        return Ok(());
    }
    check_keys(id, spec.params, params)?;
    if id == "command.batch" {
        for step in params.get("commands").and_then(Value::as_array).into_iter().flatten() {
            if let (Some(step_id), Some(step_params)) = (step.get("command").and_then(Value::as_str), step.get("params")) {
                check_params(step_id, step_params)?;
            }
        }
    }
    Ok(())
}

impl crate::Session {
    /// [`check_params`], and [`check_effect_params`] for the effects `effect.setParams` edits in
    /// this session's document (not as a step of a `command.batch`: what it edits is only there
    /// once the steps before it ran).
    pub fn check_params(&self, id: &str, params: &Value) -> Result<()> {
        check_params(id, params)?;
        if let ("effect.setParams", Some(given)) = (id, params.get("params")) {
            for effect in super::effectcmd::set_params_effects(self, params) {
                check_effect_params(id, &effect, given)?;
            }
        }
        Ok(())
    }
}

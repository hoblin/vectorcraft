//! Answers in words for the agents that drive the engine from outside ([`crate::Session::call`]):
//! what a command changed when it answers nothing itself, where a missing object is, and what the
//! ids and indices don't say.

use std::sync::Arc;

use vectorcraft_doc::{Document, Node, NodeId, NodeKind, Selection};

use crate::{DocState, cmd};

/// `` `{id}` `` plus the commands named like it, when `id` isn't a command.
pub fn unknown_command(id: &str) -> String {
    let like = similar_commands(id);
    if like.is_empty() { format!("unknown command `{id}`") } else { format!("unknown command `{id}`; commands named like it: {}", like.join(", ")) }
}

/// Command ids that contain `id`, or whose last segment starts with the first four characters of
/// its last segment: at most eight, never one guessed answer.
pub fn similar_commands(id: &str) -> Vec<&'static str> {
    let id = id.to_ascii_lowercase();
    let last = id.rsplit('.').next().unwrap_or_default();
    let head: String = last.chars().take(4).collect();
    cmd::command_specs()
        .iter()
        .filter(|c| !cmd::is_alias(c.id))
        .filter(|c| {
            let lower = c.id.to_ascii_lowercase();
            lower.contains(&id) || (head.chars().count() == 4 && lower.rsplit('.').next().is_some_and(|l| l.starts_with(&head)))
        })
        .map(|c| c.id)
        .take(8)
        .collect()
}

/// What makes a command available, after the reason it isn't: the fix in the agent's own
/// vocabulary, for the reasons that have one.
pub(crate) fn how_to_enable(spec: &cmd::CommandSpec, reason: &str) -> String {
    let acts = if cmd::params::takes_key(spec.params, "ids") {
        "It acts on `ids` or on the selected objects"
    } else if cmd::params::takes_key(spec.params, "id") {
        "It acts on `id` or on the selected objects"
    } else {
        "It acts on the selected objects"
    };
    match reason {
        "no document open" => " file.new or file.open opens one.".into(),
        "nothing selected" | "select at least two objects" => {
            format!(" {acts}; select.set {{\"ids\": […]}} selects objects, select.all every object.")
        }
        "no anchor points selected" => " select.anchors {\"id\": …, \"anchors\": [[subpath, anchor]…]} selects anchor points of a path.".into(),
        "clipboard is empty" => " edit.copy or edit.cut fills it.".into(),
        _ => String::new(),
    }
}

/// Where object `id`, which a command was given, is: not in this document, or in one of the other
/// open documents.
pub(crate) fn no_object(docs: &[DocState], active: Option<usize>, id: NodeId) -> String {
    let elsewhere = docs.iter().enumerate().filter(|(i, _)| Some(*i) != active).find(|(_, d)| d.doc.node(id).is_some());
    match elsewhere {
        Some((i, d)) => format!(
            "object {id} is in {}, document index {i}, not in the active document; document.activate {{\"index\": {i}}} makes it active",
            quoted(&d.title())
        ),
        None => format!("no object {id} in the document; document.inspect lists the objects with their ids"),
    }
}

fn quoted(name: &str) -> String {
    format!("\"{name}\"")
}

/// `rectangle #2`, as the Layers panel names the kind.
fn named(doc: &Document, id: NodeId) -> String {
    match doc.node(id) {
        Some(n) => format!("{} {id}", n.kind_label().to_ascii_lowercase()),
        None => format!("object {id}"),
    }
}

/// `rectangle #2`, `rectangle #2 and ellipse #3`, `rectangle #2, ellipse #3 and path #4`; past
/// four, `6 objects (#2, #3, #4, #5, #6, #7)`.
fn objects(doc: &Document, ids: &[NodeId]) -> String {
    const LISTED: usize = 12;
    if ids.len() > 4 {
        let shown: Vec<String> = ids.iter().take(LISTED).map(ToString::to_string).collect();
        let more = ids.len().saturating_sub(LISTED);
        let tail = if more > 0 { format!(" and {more} more") } else { String::new() };
        return format!("{} objects ({}{tail})", ids.len(), shown.join(", "));
    }
    let names: Vec<String> = ids.iter().map(|id| named(doc, *id)).collect();
    match names.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// The container `id` is in: `layer "Layer 1"` or `group #7`.
fn container(doc: &Document, id: NodeId) -> String {
    match doc.node(id) {
        Some(n) if matches!(n.kind, NodeKind::Layer { .. }) => format!("layer {}", quoted(n.name.as_deref().unwrap_or_default())),
        _ => named(doc, id),
    }
}

/// What a command did, from the active document before it (`before`: its uid, document,
/// selection and current layer) and the session after it: one sentence per kind of change,
/// joined with `; `, or `nothing changed`.
pub(crate) fn changes(before: Option<(u64, Arc<Document>, Selection, Option<NodeId>)>, after: Option<&DocState>) -> String {
    let said = match (before, after) {
        (_, None) => vec!["no document is open".to_string()],
        (Some((uid, doc, sel, layer)), Some(st)) if uid == st.uid => {
            let mut said = if Arc::ptr_eq(&doc, &st.doc) { vec![] } else { document(&doc, &st.doc) };
            said.extend(selection(&sel, &st.selection, &st.doc));
            if layer != st.active_layer
                && let Some(l) = st.active_layer
            {
                said.push(format!("the current layer is now {}", container(&st.doc, l)));
            }
            said
        }
        (_, Some(st)) => vec![format!("the active document is now {}", quoted(&st.title()))],
    };
    if said.is_empty() { "nothing changed".into() } else { said.join("; ") }
}

/// The object edits, in the order the tree changed, then the document's parts.
fn document(old: &Document, new: &Document) -> Vec<String> {
    let mut diff = Diff::default();
    diff.layers(old, new);
    let mut said = diff.said(old, new);
    let parts: Vec<&str> = [
        (old.title != new.title, "title"),
        (old.units != new.units, "units"),
        (old.color_mode != new.color_mode, "colour mode"),
        (old.ruler_origin != new.ruler_origin, "ruler origin"),
        (old.swatches != new.swatches || old.swatch_groups != new.swatch_groups, "swatches"),
        (old.graphic_styles != new.graphic_styles, "graphic styles"),
        (old.char_styles != new.char_styles, "character styles"),
        (old.para_styles != new.para_styles, "paragraph styles"),
        (old.text_threads != new.text_threads, "text threads"),
        (old.symbols != new.symbols, "symbols"),
        (old.graph_designs != new.graph_designs, "graph designs"),
        (old.guides != new.guides, "guides"),
        (old.views != new.views, "saved views"),
        (old.saved_selections != new.saved_selections, "saved selections"),
        (old.variables != new.variables, "variables"),
        (old.grid != new.grid, "grid"),
        (old.raster_effects_ppi != new.raster_effects_ppi || old.raster_effects != new.raster_effects, "raster effects settings"),
        (old.patterns != new.patterns, "patterns"),
        (old.pattern_edit != new.pattern_edit, "pattern editing mode"),
        (old.mask_edit != new.mask_edit, "opacity mask editing mode"),
        (old.page_isolate != new.page_isolate || old.page_knockout != new.page_knockout, "page transparency"),
        (old.spot_use_lab != new.spot_use_lab, "spot colour setting"),
        (old.setup != new.setup, "document setup"),
        (old.paste_remembers_layers != new.paste_remembers_layers, "Paste Remembers Layers"),
        (old.metadata != new.metadata, "file info"),
        (old.color_profiles != new.color_profiles, "colour profiles"),
        (old.export_settings != new.export_settings, "export settings"),
        (old.slices != new.slices || old.slices_clip_to_artboard != new.slices_clip_to_artboard, "slices"),
        (old.print_setup != new.print_setup, "print settings"),
        (old.assets != new.assets, "export assets"),
        (old.template != new.template, "template flag"),
    ]
    .into_iter()
    .filter_map(|(changed, name)| changed.then_some(name))
    .collect();
    if !parts.is_empty() {
        said.push(format!("changed the document's {}", parts.join(", ")));
    }
    said.extend(artboards(old, new));
    said
}

fn artboards(old: &Document, new: &Document) -> Vec<String> {
    let mut said = vec![];
    let indices = |v: &[usize]| v.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
    let missing = |a: &Document, b: &Document| -> Vec<usize> {
        a.artboards.iter().enumerate().filter(|(_, x)| !b.artboards.iter().any(|y| y.id == x.id)).map(|(i, _)| i).collect()
    };
    let (added, deleted) = (missing(new, old), missing(old, new));
    let changed: Vec<usize> =
        new.artboards.iter().enumerate().filter(|(_, x)| old.artboards.iter().any(|y| y.id == x.id && y != *x)).map(|(i, _)| i).collect();
    for (verb, list, note) in [("added", &added, ""), ("deleted", &deleted, " (as numbered before)"), ("changed", &changed, "")] {
        match list.len() {
            0 => {}
            1 => said.push(format!("{verb} artboard index {}{note}", indices(list))),
            _ => said.push(format!("{verb} artboards at index {}{note}", indices(list))),
        }
    }
    let kept = |a: &Document, b: &Document| a.artboards.iter().map(|x| x.id).filter(|id| b.artboards.iter().any(|y| y.id == *id)).collect::<Vec<_>>();
    if kept(old, new) != kept(new, old) {
        said.push("reordered the artboards".into());
    }
    said
}

/// The object changes between two trees, found without visiting what both trees share.
#[derive(Default)]
struct Diff {
    added: Vec<(NodeId, NodeId)>,
    deleted: Vec<NodeId>,
    moved: Vec<(NodeId, NodeId)>,
    changed: Vec<(NodeId, Vec<&'static str>)>,
    reordered: Vec<NodeId>,
}

impl Diff {
    fn layers(&mut self, old: &Document, new: &Document) {
        self.list(old, new, None, &old.layers, &new.layers);
    }

    /// The changes between the children `old` and `new` of container `parent` (`None`: the
    /// document's layers).
    fn list(&mut self, old_doc: &Document, new_doc: &Document, parent: Option<NodeId>, old: &[Arc<Node>], new: &[Arc<Node>]) {
        for n in new {
            match old.iter().find(|o| o.id == n.id) {
                None if old_doc.node(n.id).is_some() => {
                    if let Some(p) = parent {
                        self.moved.push((n.id, p));
                    }
                }
                None => self.added.push((n.id, parent.unwrap_or(n.id))),
                Some(o) if Arc::ptr_eq(o, n) || o == n => {}
                Some(o) => self.node(old_doc, new_doc, o, n),
            }
        }
        for o in old {
            if !new.iter().any(|n| n.id == o.id) && new_doc.node(o.id).is_none() {
                self.deleted.push(o.id);
            }
        }
        let kept = |a: &[Arc<Node>], b: &[Arc<Node>]| a.iter().map(|x| x.id).filter(|id| b.iter().any(|y| y.id == *id)).collect::<Vec<_>>();
        if kept(old, new) != kept(new, old)
            && let Some(p) = parent
        {
            self.reordered.push(p);
        }
    }

    /// One node present in both trees and different: what of its own changed, or, for a container
    /// whose own parts are the same, its children (all of them: the container; some: those).
    fn node(&mut self, old_doc: &Document, new_doc: &Document, old: &Node, new: &Node) {
        let own = own_changes(old, new);
        let is_layer = matches!(new.kind, NodeKind::Layer { .. });
        match (old.children(), new.children()) {
            (Some(oc), Some(nc)) => {
                let same = |o: &Arc<Node>, n: &Arc<Node>| o.id == n.id && (Arc::ptr_eq(o, n) || o == n);
                let members_changed = nc.iter().filter(|n| !oc.iter().any(|o| same(o, n))).count();
                let same_members = oc.len() == nc.len() && nc.iter().all(|n| oc.iter().any(|o| o.id == n.id));
                if !is_layer && same_members && members_changed == nc.len() && !nc.is_empty() {
                    self.changed.push((new.id, own));
                } else {
                    if !own.is_empty() || (!is_layer && members_changed == 0) {
                        self.changed.push((new.id, own));
                    }
                    self.list(old_doc, new_doc, Some(new.id), oc, nc);
                }
            }
            _ => self.changed.push((new.id, own)),
        }
    }

    fn said(self, old: &Document, doc: &Document) -> Vec<String> {
        let mut said = vec![];
        let mut by_place: Vec<(NodeId, Vec<NodeId>)> = vec![];
        for (id, place) in self.added {
            match by_place.iter_mut().find(|(p, _)| *p == place) {
                Some((_, ids)) => ids.push(id),
                None => by_place.push((place, vec![id])),
            }
        }
        for (place, ids) in by_place {
            match ids.first() {
                Some(first) if *first == place => said.push(format!("added {}", objects(doc, &ids))),
                _ => said.push(format!("added {} to {}", objects(doc, &ids), container(doc, place))),
            }
        }
        if !self.deleted.is_empty() {
            said.push(format!("deleted {}", objects(old, &self.deleted)));
        }
        let mut by_place: Vec<(NodeId, Vec<NodeId>)> = vec![];
        for (id, place) in self.moved {
            match by_place.iter_mut().find(|(p, _)| *p == place) {
                Some((_, ids)) => ids.push(id),
                None => by_place.push((place, vec![id])),
            }
        }
        for (place, ids) in by_place {
            said.push(format!("moved {} to {}", objects(doc, &ids), container(doc, place)));
        }
        let mut by_parts: Vec<(Vec<&'static str>, Vec<NodeId>)> = vec![];
        for (id, parts) in self.changed {
            match by_parts.iter_mut().find(|(p, _)| *p == parts) {
                Some((_, ids)) => ids.push(id),
                None => by_parts.push((parts, vec![id])),
            }
        }
        for (parts, ids) in by_parts {
            match parts.split_last() {
                None => said.push(format!("changed {}", objects(doc, &ids))),
                Some((last, [])) => said.push(format!("changed the {last} of {}", objects(doc, &ids))),
                Some((last, rest)) => said.push(format!("changed the {} and {last} of {}", rest.join(", "), objects(doc, &ids))),
            }
        }
        for place in self.reordered {
            said.push(format!("reordered the objects in {}", container(doc, place)));
        }
        said
    }
}

/// The parts of `new` that differ from `old`, children aside, named as the panels name them.
fn own_changes(old: &Node, new: &Node) -> Vec<&'static str> {
    let contents = match (&old.kind, &new.kind) {
        (NodeKind::Path { .. }, NodeKind::Path { .. }) => "shape",
        (NodeKind::Text(a), NodeKind::Text(b)) => text_change(a, b),
        (NodeKind::Image(_), NodeKind::Image(_)) => "image",
        _ => "contents",
    };
    let kind_changed = match (old.children(), new.children()) {
        (Some(_), Some(_)) => false,
        _ => old.kind != new.kind,
    };
    [
        (old.name != new.name, "name"),
        (old.visible != new.visible, "visibility"),
        (old.locked != new.locked, "lock"),
        (old.opacity != new.opacity, "opacity"),
        (old.blend != new.blend, "blend mode"),
        (old.appearance.fill() != new.appearance.fill(), "fill"),
        (old.appearance.stroke() != new.appearance.stroke(), "stroke"),
        (old.appearance.effects != new.appearance.effects, "effects"),
        (
            old.appearance.fill() == new.appearance.fill()
                && old.appearance.stroke() == new.appearance.stroke()
                && old.appearance.effects == new.appearance.effects
                && old.appearance != new.appearance,
            "appearance",
        ),
        (old.extras() != new.extras(), "attributes"),
        (old.bbox_angle != new.bbox_angle, "bounding box angle"),
        (kind_changed, contents),
    ]
    .into_iter()
    .filter_map(|(changed, name)| changed.then_some(name))
    .collect()
}

/// What of a type object changed: its text, else the paint of its characters (type carries its
/// fill and stroke in its runs, not in its appearance), its place, else its formatting.
fn text_change(old: &vectorcraft_doc::TextObject, new: &vectorcraft_doc::TextObject) -> &'static str {
    let paint = |t: &vectorcraft_doc::TextObject, f: fn(&vectorcraft_doc::CharStyle) -> vectorcraft_color::Paint| {
        t.runs.iter().map(|r| f(&r.style)).collect::<Vec<_>>()
    };
    if old.plain_text() != new.plain_text() {
        "text"
    } else if paint(old, |s| s.fill.clone()) != paint(new, |s| s.fill.clone()) {
        "fill"
    } else if paint(old, |s| s.stroke.clone()) != paint(new, |s| s.stroke.clone())
        || old.runs.iter().map(|r| r.style.stroke_width).ne(new.runs.iter().map(|r| r.style.stroke_width))
    {
        "stroke"
    } else if old.xf != new.xf {
        "position"
    } else {
        "formatting"
    }
}

fn selection(old: &Selection, new: &Selection, doc: &Document) -> Vec<String> {
    let mut said = vec![];
    if old.objects != new.objects {
        said.push(if new.objects.is_empty() { "no object is selected".into() } else { format!("selected {}", objects(doc, &new.objects)) });
    }
    if old.anchors != new.anchors {
        let points: usize = new.anchors.values().map(|a| a.len()).sum();
        let paths: Vec<NodeId> = new.anchors.iter().filter(|(_, a)| !a.is_empty()).map(|(id, _)| *id).collect();
        said.push(match points {
            0 => "no anchor point is selected".into(),
            1 => format!("selected 1 anchor point of {}", objects(doc, &paths)),
            n => format!("selected {n} anchor points of {}", objects(doc, &paths)),
        });
    }
    if old.key != new.key
        && let Some(key) = new.key
    {
        said.push(format!("the key object is {}", named(doc, key)));
    }
    if old.guides != new.guides {
        said.push(match new.guides.len() {
            0 => "no guide is selected".into(),
            1 => "selected 1 guide".into(),
            n => format!("selected {n} guides"),
        });
    }
    said
}

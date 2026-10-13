//! [`Session::call`] answers in words: what a command changed, what an error needs next.

use serde_json::{Value, json};

use crate::{EngineError, Session, command_specs};

fn drawn() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 200, "height": 100})).unwrap();
    s.execute("shape.rectangle", &json!({"x": 10, "y": 10, "width": 50, "height": 40})).unwrap();
    s
}

/// What `call` says: a sentence, another answer as JSON, or the error.
fn said(s: &mut Session, id: &str, params: Value) -> String {
    match s.call(id, &params) {
        Ok(Value::String(said)) => said,
        Ok(other) => other.to_string(),
        Err(e) => e.to_string(),
    }
}

#[test]
fn a_call_that_answers_nothing_says_what_changed() {
    let mut s = drawn();
    assert_eq!(said(&mut s, "paint.setFill", json!({"color": "#ff0000"})), "changed the fill of rectangle #2");
    assert_eq!(
        said(&mut s, "object.setProps", json!({"name": "Box", "locked": true})),
        "changed the name and lock of rectangle #2; no object is selected"
    );
    assert_eq!(said(&mut s, "select.none", json!({})), "nothing changed");
    assert_eq!(said(&mut s, "stroke.set", json!({"width": 3})), "nothing changed: no object is selected and no `ids` were given");
    assert_eq!(said(&mut s, "transparency.set", json!({"ids": [2], "opacity": 50})), "changed the opacity of rectangle #2");
    s.call("text.create", &json!({"x": 10, "y": 80, "text": "hi"})).unwrap();
    assert_eq!(said(&mut s, "paint.setFill", json!({"color": "#0047bb"})), "changed the fill of type #3");
    assert_eq!(said(&mut s, "object.move", json!({"dx": 5, "dy": 5})), r#"{"ids":[3]}"#);
    assert_eq!(said(&mut s, "artboard.new", json!({})), r#"{"index":1}"#);
    assert_eq!(said(&mut s, "artboard.delete", json!({"index": 1})), "deleted artboard index 1 (as numbered before)");
    assert_eq!(said(&mut s, "file.close", json!({})), "no document is open");
}

#[test]
fn a_group_edit_names_the_group_and_an_ungroup_its_members() {
    let mut s = drawn();
    s.call("shape.ellipse", &json!({"x": 10, "y": 10, "width": 50, "height": 40})).unwrap();
    s.call("select.all", &json!({})).unwrap();
    let group = s.call("object.group", &json!({})).unwrap()["id"].clone();
    assert_eq!(said(&mut s, "object.hide", json!({})), "changed the visibility of group #4; no object is selected");
    s.call("object.showAll", &json!({})).unwrap();
    s.call("select.set", &json!({"ids": [group]})).unwrap();
    assert_eq!(
        said(&mut s, "object.ungroup", json!({})),
        "deleted group #4; moved rectangle #2 and ellipse #3 to layer \"Layer 1\"; selected rectangle #2 and ellipse #3"
    );
    assert_eq!(said(&mut s, "edit.clear", json!({})), "deleted rectangle #2 and ellipse #3; no object is selected");
}

#[test]
fn a_missing_object_is_reported_with_where_to_look() {
    let mut s = drawn();
    assert_eq!(
        said(&mut s, "edit.clear", json!({"ids": [999]})),
        "invalid parameters for `edit.clear`: no object #999 in the document; document.inspect lists the objects with their ids; `edit.clear` takes {ids?}"
    );
    s.call("file.new", &json!({})).unwrap();
    assert_eq!(
        said(&mut s, "select.set", json!({"ids": [2]})),
        "invalid parameters for `select.set`: object #2 is in \"Untitled-1\", document index 0, not in the active document; document.activate {\"index\": 0} makes it active; `select.set` takes {ids}"
    );
}

#[test]
fn an_error_says_what_the_command_takes_or_what_makes_it_available() {
    let mut s = drawn();
    assert_eq!(
        said(&mut s, "shape.rectangle", json!({"x": 1, "y": 1, "width": 5})),
        "invalid parameters for `shape.rectangle`: missing number `height`; `shape.rectangle` takes {x, y, width, height, radius?, rotation?}"
    );
    s.call("select.none", &json!({})).unwrap();
    assert_eq!(
        said(&mut s, "object.group", json!({})),
        "command `object.group` is not available right now: nothing selected. It acts on the selected objects; select.set {\"ids\": […]} selects objects, select.all every object."
    );
    assert_eq!(
        said(&mut s, "edit.clear", json!({})),
        "command `edit.clear` is not available right now: nothing selected. It acts on `ids` or on the selected objects; select.set {\"ids\": […]} selects objects, select.all every object."
    );
    assert_eq!(
        said(&mut s, "edit.paste", json!({})),
        "command `edit.paste` is not available right now: clipboard is empty. edit.copy or edit.cut fills it."
    );
    assert_eq!(said(&mut s, "artboard.delete", json!({"index": 5})), "no artboard 5 (0-based; the document has 1)");
    assert!(
        said(&mut s, "shape.rectangel", json!({}))
            .starts_with("unknown command `shape.rectangel`; commands named like it: shape.rectangle, shape.rectangularGrid")
    );
    assert_eq!(said(&mut s, "shape.siz", json!({})), "unknown command `shape.siz`");
    assert!(said(&mut s, "setFill", json!({})).starts_with("unknown command `setFill`; commands named like it: paint.setFill"));
    assert_eq!(said(&mut s, "zzz", json!({})), "unknown command `zzz`");
    let mut s = Session::new();
    assert_eq!(
        said(&mut s, "select.all", json!({})),
        "command `select.all` is not available right now: no document open. file.new or file.open opens one."
    );
}

/// What makes a command available names `ids` only when the command takes them.
#[test]
fn how_to_enable_names_only_keys_the_command_takes() {
    let mut s = drawn();
    s.call("select.none", &json!({})).unwrap();
    for c in command_specs() {
        let Err(EngineError::Disabled(_, why)) = s.call(c.id, &json!({})) else { continue };
        if why.contains("It acts on `ids`") {
            assert!(crate::cmd::params::takes_key(c.params, "ids"), "{}: {why}", c.id);
        }
    }
}

#[test]
fn a_font_that_is_not_installed_is_said_so() {
    let mut s = drawn();
    let r = s.call("text.create", &json!({"x": 10, "y": 80, "text": "hi", "font": "No Such Family"})).unwrap();
    assert_eq!(
        r["warning"],
        format!(
            "font \"No Such Family\" isn't installed; the type is drawn in {} (text.fonts lists the fonts there are)",
            vectorcraft_text::FALLBACK_FAMILY
        )
    );
    let r = s.call("text.create", &json!({"x": 10, "y": 80, "text": "hi", "font": vectorcraft_text::FALLBACK_FAMILY})).unwrap();
    assert!(r.get("warning").is_none(), "{r}");
}

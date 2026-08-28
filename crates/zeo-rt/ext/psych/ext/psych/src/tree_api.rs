//! The bridge between the Rust [`Node`] tree and psych's `Psych::Nodes::*`
//! Ruby objects.
//!
//! Two directions, and both are needed:
//!
//!   * [`to_ruby_nodes`] mirrors a parsed tree into Ruby objects, which is
//!     what `Psych.parse` answers.
//!   * [`from_ruby_nodes`] reads those objects back into a [`Node`], which is
//!     what `Nodes::Node#to_ruby` walks.
//!
//! The second direction is not just the inverse of the first. A program is
//! allowed to BUILD a tree by hand -- `Nodes::Mapping.new` with children
//! pushed onto it -- and then call `to_ruby` on it. Reading the Ruby objects
//! rather than keeping a hidden Rust handle beside them is what makes a
//! hand-built tree work exactly like a parsed one.

use super::nodes::{Document, Node, style};
use crate::dispatch::{raise_error, send_value};
use crate::{RubyValue, Signal, Symbol, string_new};

fn nodes_class(name: &str) -> Result<RubyValue, Signal> {
    super::loader::class_named(&format!("Psych::Nodes::{name}"))
}

fn s(text: &str) -> RubyValue {
    RubyValue::Str(string_new(text.to_string()))
}

fn opt_str(text: Option<&str>) -> RubyValue {
    match text {
        Some(t) => s(t),
        None => RubyValue::Nil,
    }
}

fn set_ivar(obj: &RubyValue, name: &str, v: RubyValue) -> Result<(), Signal> {
    send_value(
        obj,
        Symbol::intern("instance_variable_set"),
        &[s(name), v],
        None,
    )?;
    Ok(())
}

fn get_ivar(obj: &RubyValue, name: &str) -> Result<RubyValue, Signal> {
    send_value(obj, Symbol::intern("instance_variable_get"), &[s(name)], None)
}

fn push_child(parent: &RubyValue, child: RubyValue) -> Result<(), Signal> {
    let children = get_ivar(parent, "@children")?;
    if let RubyValue::Array(a) = children {
        a.lock().push(child);
    }
    Ok(())
}

// ---- Rust tree -> Ruby objects --------------------------------------------

/// Every document as a `Psych::Nodes::Document`.
pub(super) fn to_ruby_documents(docs: &[Document]) -> Result<Vec<RubyValue>, Signal> {
    docs.iter().map(document_object).collect()
}

/// The whole stream as a `Psych::Nodes::Stream`, which is what
/// `Psych.parse_stream` answers.
pub(super) fn to_ruby_stream(docs: &[Document]) -> Result<RubyValue, Signal> {
    let cls = nodes_class("Stream")?;
    let stream = send_value(&cls, Symbol::intern("new"), &[], None)?;
    for doc in docs {
        push_child(&stream, document_object(doc)?)?;
    }
    Ok(stream)
}

fn document_object(doc: &Document) -> Result<RubyValue, Signal> {
    let version = match doc.version {
        Some((major, minor)) => RubyValue::Array(crate::collections::array_new(vec![
            RubyValue::Int(major),
            RubyValue::Int(minor),
        ])),
        None => RubyValue::Array(crate::collections::array_new(Vec::new())),
    };
    let directives: Vec<RubyValue> = doc
        .tag_directives
        .iter()
        .map(|(handle, prefix)| {
            RubyValue::Array(crate::collections::array_new(vec![s(handle), s(prefix)]))
        })
        .collect();
    let cls = nodes_class("Document")?;
    let obj = send_value(
        &cls,
        Symbol::intern("new"),
        &[
            version,
            RubyValue::Array(crate::collections::array_new(directives)),
            RubyValue::Bool(doc.implicit),
        ],
        None,
    )?;
    set_ivar(&obj, "@implicit_end", RubyValue::Bool(doc.implicit_end))?;
    if let Some(root) = &doc.root {
        push_child(&obj, to_ruby_node(root)?)?;
    }
    Ok(obj)
}

fn to_ruby_node(node: &Node) -> Result<RubyValue, Signal> {
    match node {
        Node::Scalar {
            value,
            style: sty,
            quoted,
            tag,
            anchor,
        } => {
            let cls = nodes_class("Scalar")?;
            // `plain` and `quoted` are the two booleans a gem reads instead
            // of the style number, and they are not each other's inverse only
            // for a node nothing parsed.
            let plain = !*quoted;
            let quoted = *quoted;
            send_value(
                &cls,
                Symbol::intern("new"),
                &[
                    s(value),
                    opt_str(anchor.as_deref()),
                    opt_str(tag.as_deref()),
                    RubyValue::Bool(plain),
                    RubyValue::Bool(quoted),
                    RubyValue::Int(*sty),
                ],
                None,
            )
        }
        Node::Sequence {
            children,
            style: sty,
            tag,
            anchor,
        } => container_object("Sequence", children, *sty, tag.as_deref(), anchor.as_deref()),
        Node::Mapping {
            children,
            style: sty,
            tag,
            anchor,
        } => container_object("Mapping", children, *sty, tag.as_deref(), anchor.as_deref()),
        Node::Alias { anchor } => {
            let cls = nodes_class("Alias")?;
            send_value(&cls, Symbol::intern("new"), &[s(anchor)], None)
        }
    }
}

fn container_object(
    class: &str,
    children: &[Node],
    sty: i64,
    tag: Option<&str>,
    anchor: Option<&str>,
) -> Result<RubyValue, Signal> {
    let cls = nodes_class(class)?;
    let obj = send_value(
        &cls,
        Symbol::intern("new"),
        &[
            opt_str(anchor),
            opt_str(tag),
            // `implicit` means the container carried no explicit tag.
            RubyValue::Bool(tag.is_none()),
            RubyValue::Int(sty),
        ],
        None,
    )?;
    for child in children {
        push_child(&obj, to_ruby_node(child)?)?;
    }
    Ok(obj)
}

// ---- Ruby objects -> Rust tree --------------------------------------------

/// A `Psych::Nodes::*` object read back into a [`Node`].
///
/// A `Stream` or a `Document` unwraps to its root, so `to_ruby` on any of the
/// three answers the document's value -- which is what psych does and what a
/// caller of `Psych.parse(str).to_ruby` expects.
pub(super) fn node_kind(v: &RubyValue) -> String {
    crate::dispatch::class_name(v.class_id())
        .unwrap_or_default()
        .rsplit("::")
        .next()
        .unwrap_or("")
        .to_string()
}

/// Every document under a `Nodes::Stream`, as roots.
///
/// A Stream's `to_ruby` answers an ARRAY -- one value per document -- rather
/// than the first document's value. Measured: `Psych.parse_stream("--- 7\n")
/// .to_ruby` is `[7]`, not `7`.
pub(super) fn stream_roots(v: &RubyValue) -> Result<Vec<Node>, Signal> {
    let RubyValue::Array(a) = get_ivar(v, "@children")? else {
        return Ok(Vec::new());
    };
    let docs: Vec<RubyValue> = a.lock().iter().cloned().collect();
    let mut out = Vec::new();
    for doc in &docs {
        if let Some(node) = from_ruby_nodes(doc)? {
            out.push(node);
        }
    }
    Ok(out)
}

pub(super) fn from_ruby_nodes(v: &RubyValue) -> Result<Option<Node>, Signal> {
    match node_kind(v).as_str() {
        "Stream" | "Document" => match first_child(v)? {
            Some(root) => from_ruby_nodes(&root),
            None => Ok(None),
        },
        "Scalar" => Ok(Some(Node::Scalar {
            value: text_of(&get_ivar(v, "@value")?),
            style: int_of(&get_ivar(v, "@style")?, style::ANY),
            // A node built by hand carries `quoted: false` and a style of
            // ANY, and psych scans its text -- so this reads the FLAG and
            // never infers one from the style.
            quoted: matches!(get_ivar(v, "@quoted")?, RubyValue::Bool(true)),
            tag: opt_text(&get_ivar(v, "@tag")?),
            anchor: opt_text(&get_ivar(v, "@anchor")?),
        })),
        "Sequence" => Ok(Some(Node::Sequence {
            children: children_of(v)?,
            style: int_of(&get_ivar(v, "@style")?, style::BLOCK),
            tag: opt_text(&get_ivar(v, "@tag")?),
            anchor: opt_text(&get_ivar(v, "@anchor")?),
        })),
        "Mapping" => Ok(Some(Node::Mapping {
            children: children_of(v)?,
            style: int_of(&get_ivar(v, "@style")?, style::BLOCK),
            tag: opt_text(&get_ivar(v, "@tag")?),
            anchor: opt_text(&get_ivar(v, "@anchor")?),
        })),
        "Alias" => Ok(Some(Node::Alias {
            anchor: text_of(&get_ivar(v, "@anchor")?),
        })),
        other => Err(raise_error(
            "TypeError",
            format!("expected a Psych::Nodes node, got {other}"),
        )),
    }
}

fn first_child(v: &RubyValue) -> Result<Option<RubyValue>, Signal> {
    let RubyValue::Array(a) = get_ivar(v, "@children")? else {
        return Ok(None);
    };
    let first = a.lock().first().cloned();
    Ok(first)
}

fn children_of(v: &RubyValue) -> Result<Vec<Node>, Signal> {
    let RubyValue::Array(a) = get_ivar(v, "@children")? else {
        return Ok(Vec::new());
    };
    let items: Vec<RubyValue> = a.lock().iter().cloned().collect();
    let mut out = Vec::new();
    for item in &items {
        if let Some(node) = from_ruby_nodes(item)? {
            out.push(node);
        }
    }
    Ok(out)
}

/// A scalar's `@value` is a String, but a hand-built node may hold anything
/// that prints -- psych stringifies it on the way out, so this does too.
fn text_of(v: &RubyValue) -> String {
    match v {
        RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
        RubyValue::Nil => String::new(),
        other => other.to_display_string(),
    }
}

fn opt_text(v: &RubyValue) -> Option<String> {
    match v {
        RubyValue::Nil => None,
        other => {
            let t = text_of(other);
            // A hand-built node carries `""` where a parsed one carries nil,
            // and an empty tag is not a tag.
            if t.is_empty() { None } else { Some(t) }
        }
    }
}

fn int_of(v: &RubyValue, fallback: i64) -> i64 {
    match v {
        RubyValue::Int(n) => *n,
        _ => fallback,
    }
}

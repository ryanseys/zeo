//! Constant-name and constant-path helpers: a plain constant name, a
//! `Foo::Bar` path joined into one string, its `(scope, name)` split for a
//! `ConstWrite`/`QualifiedConstRead`, and a `box::A::B` path rooted at a
//! local box handle. Split out of `parse/mod.rs`.

use super::{PResult, context};
use ruby_prism::Node;

fn constant_name(node: &Node<'_>) -> PResult<String> {
    let cr = node
        .as_constant_read_node()
        .ok_or("expected a plain constant name (e.g. `Foo`, not `Foo::Bar`)")?;
    Ok(String::from_utf8_lossy(cr.name().as_slice()).into_owned())
}

/// A constant PATH wherever a class/module is being NAMED:
/// definitions (`class Store::Item`), superclasses, include/extend/prepend
/// targets, `.new` receivers, `rescue` lists, and pattern constants.
/// Produces the joined `"A::B::C"` form `Compiler::resolve_class` takes
/// apart again; a top-level-anchored `::Foo` keeps its leading `::` (the
/// anchor skips the lexical chain at resolution time). A dynamic parent
/// (`something::Foo` where `something` isn't itself a constant) stays a
/// clean rejection.
pub fn constant_path_name(node: &Node<'_>) -> PResult<String> {
    if node.as_constant_read_node().is_some() {
        return constant_name(node);
    }
    let cp = node
        .as_constant_path_node()
        .ok_or("expected a constant name or path (e.g. `Foo` or `Foo::Bar`)")?;
    let name = cp.name().ok_or(
        "a `::` constant path with a dynamic/computed name isn't supported (zeo limitation)",
    )?;
    let name = String::from_utf8_lossy(name.as_slice()).into_owned();
    Ok(match cp.parent() {
        None => format!("::{name}"),
        Some(p) => format!("{}::{}", constant_path_name(&p)?, name),
    })
}

/// `Foo::BAR` / `Foo::Bar::BAZ` (`ConstantPathNode`) -- resolves to
/// `(scope path, name)` for a `HirNode::ConstWrite`/`QualifiedConstRead`'s
/// fields: the LAST segment is the constant being read/written, everything
/// before it is the owning class/module path (multi-segment since Phase
/// 15.3, resolved by `Compiler::resolve_class`). `::FOO` (no `parent` at
/// all -- an explicit top-level anchor) resolves against `Object` directly,
/// mirroring real Ruby's own representation of top-level constants as
/// living on `Object`.
pub(crate) fn constant_path_scope_and_name(
    node: &ruby_prism::ConstantPathNode<'_>,
) -> PResult<(String, String)> {
    let name = node.name().ok_or(
        "a `::` constant path with a dynamic/computed name isn't supported (zeo limitation)",
    )?;
    let name = String::from_utf8_lossy(name.as_slice()).into_owned();
    let scope = match node.parent() {
        None => "Object".to_string(),
        Some(p) => constant_path_name(&p)?,
    };
    Ok((scope, name))
}

/// `box::A::B` -- a constant path rooted at a LOCAL bound to a box handle
///. Returns the box id plus the path INSIDE the box (`"A::B"`).
pub(crate) fn box_rooted_path(node: &Node<'_>) -> Option<(u32, String)> {
    let cp = node.as_constant_path_node()?;
    let name = cp.name()?;
    let name = String::from_utf8_lossy(name.as_slice()).into_owned();
    let parent = cp.parent()?;
    if let Some(lv) = parent.as_local_variable_read_node() {
        let lname = String::from_utf8_lossy(lv.name().as_slice()).into_owned();
        let bx = context::current_box_binding(&lname)?;
        return Some((bx, name));
    }
    let (bx, prefix) = box_rooted_path(&parent)?;
    Some((bx, format!("{prefix}::{name}")))
}

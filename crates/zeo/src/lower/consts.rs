//! Constant-name and constant-path helpers: a plain constant name, a
//! `Foo::Bar` path joined into one string, its `(scope, name)` split for a
//! `ConstWrite`/`QualifiedConstRead`, and a `box::A::B` path rooted at a
//! local box handle. Split out of `parse/mod.rs`.

use super::assign::{
    Storage, bind_dynamic_const_scope, lower_and_write, lower_compound_op_write, lower_or_write,
};
use super::{PResult, context, defs, lower_node};
use crate::hir::{Hir, HirNode, NodeId};
use ruby_prism::{Node, ParseResult};

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
/// before it is the owning class/module path (multi-segment, resolved by
/// `Compiler::resolve_class`). `::FOO` (no `parent` at
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

/// A constant path whose SCOPE is an ordinary expression rather than a
/// resolvable constant path -- `self::OPTION_NAMES` in a `Struct.new` block,
/// `adapter::GitExecuteError` on a parameter, `self.class::Reason`. Returns
/// `(the scope expression, the constant's name)`, or `None` for the static
/// forms `constant_path_scope_and_name` already handles (including `::NAME`,
/// which anchors at `Object`).
pub(crate) fn dynamic_const_scope<'pr>(
    node: &ruby_prism::ConstantPathNode<'pr>,
) -> PResult<Option<(Node<'pr>, String)>> {
    let Some(parent) = node.parent() else {
        return Ok(None);
    };
    if constant_path_name(&parent).is_ok() {
        return Ok(None);
    }
    let name = node.name().ok_or(
        "a `::` constant path with a dynamic/computed name isn't supported (zeo limitation)",
    )?;
    let name = String::from_utf8_lossy(name.as_slice()).into_owned();
    Ok(Some((parent, name)))
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

/// The constant family of [`super::lower_node_inner`]'s recognizer chain:
/// bare reads, qualified paths (static, box-rooted, and dynamic-scope),
/// plain writes (including the `Struct.new`/`Module.new` class syntheses),
/// and every compound form over both spellings. `Ok(None)` = not this
/// family's node.
pub(crate) fn try_lower(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
) -> PResult<Option<NodeId>> {
    if let Some(op) = node.as_constant_operator_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice()).into_owned();
        let op_name = String::from_utf8_lossy(op.binary_operator().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(Some(lower_compound_op_write(
            hir,
            Storage::Const { scope: None, name },
            op_name,
            rhs,
        )));
    }
    if let Some(op) = node.as_constant_and_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(Some(lower_and_write(
            hir,
            Storage::Const { scope: None, name },
            rhs,
        )));
    }
    // A `# shareable_constant_value:` magic comment makes prism wrap the
    // constant write in a `ShareableConstantNode`. Zeo enforces no Ractor
    // sharing, so unwrap to the inner write and lower it verbatim.
    if let Some(sc) = node.as_shareable_constant_node() {
        return lower_node(result, hir, &sc.write()).map(Some);
    }
    if let Some(op) = node.as_constant_or_write_node() {
        let name = String::from_utf8_lossy(op.name().as_slice()).into_owned();
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(Some(lower_or_write(
            hir,
            Storage::Const { scope: None, name },
            rhs,
        )));
    }
    if let Some(cw) = node.as_constant_write_node() {
        let name = String::from_utf8_lossy(cw.name().as_slice()).into_owned();
        // `Name = Struct.new(:a, :b)` with a literal member list compiles to a
        // REAL class, so a member is a struct field an accessor reaches
        // directly instead of an overlay slot reached through a dynamic send.
        if let Some(members) = defs::as_compiled_struct(&cw.value()) {
            return defs::synthesize_struct_class(hir, &name, &members).map(Some);
        }
        // `Name = Module.new { <definitions> }` compiles to the `module Name`
        // it is equivalent to, so a later `include Name` splices a static MRO
        // edge. The synthesis declines a body whose meaning would move with the
        // cref, and that body falls through to the runtime path below.
        if let Some(body) = defs::as_synthesized_module(&cw.value())
            && let Some(id) = defs::synthesize_module(result, hir, &name, body)?
        {
            return Ok(Some(id));
        }
        // Everything else stays the RUNTIME path: the `Struct.new`/`Data.define`
        // call MINTS a class (`rstruct::struct_new`), the write binds it to the
        // constant, and `const_set` names the freshly anonymous class (Ruby's
        // "assigning an anonymous class to a constant names it").
        let value = lower_node(result, hir, &cw.value())?;
        return Ok(Some(hir.push(HirNode::ConstWrite {
            scope: None,
            name,
            value,
        })));
    }
    // `Foo::BAR` / `Foo::BAR = v` / `Foo::BAR += v` / `Foo::BAR ||= v` /
    // `Foo::BAR &&= v` -- an explicitly namespace-qualified constant
    // (`ConstantPathNode` and its write/operator-write/and-write/or-write
    // relatives). See `constant_path_scope_and_name`'s docs.
    if let Some(op) = node.as_constant_path_operator_write_node() {
        let target = op.target();
        let op_name = String::from_utf8_lossy(op.binary_operator().as_slice()).into_owned();
        if let Some((parent, name)) = dynamic_const_scope(&target)? {
            let (bind, storage) = bind_dynamic_const_scope(result, hir, &parent, name)?;
            let rhs = lower_node(result, hir, &op.value())?;
            let write = lower_compound_op_write(hir, storage, op_name, rhs);
            return Ok(Some(hir.push(HirNode::Seq(vec![bind, write]))));
        }
        let (scope, name) = constant_path_scope_and_name(&target)?;
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(Some(lower_compound_op_write(
            hir,
            Storage::Const {
                scope: Some(scope),
                name,
            },
            op_name,
            rhs,
        )));
    }
    if let Some(op) = node.as_constant_path_and_write_node() {
        let target = op.target();
        if let Some((parent, name)) = dynamic_const_scope(&target)? {
            let (bind, storage) = bind_dynamic_const_scope(result, hir, &parent, name)?;
            let rhs = lower_node(result, hir, &op.value())?;
            let write = lower_and_write(hir, storage, rhs);
            return Ok(Some(hir.push(HirNode::Seq(vec![bind, write]))));
        }
        let (scope, name) = constant_path_scope_and_name(&target)?;
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(Some(lower_and_write(
            hir,
            Storage::Const {
                scope: Some(scope),
                name,
            },
            rhs,
        )));
    }
    if let Some(op) = node.as_constant_path_or_write_node() {
        let target = op.target();
        if let Some((parent, name)) = dynamic_const_scope(&target)? {
            let (bind, storage) = bind_dynamic_const_scope(result, hir, &parent, name)?;
            let rhs = lower_node(result, hir, &op.value())?;
            let write = lower_or_write(hir, storage, rhs);
            return Ok(Some(hir.push(HirNode::Seq(vec![bind, write]))));
        }
        let (scope, name) = constant_path_scope_and_name(&target)?;
        let rhs = lower_node(result, hir, &op.value())?;
        return Ok(Some(lower_or_write(
            hir,
            Storage::Const {
                scope: Some(scope),
                name,
            },
            rhs,
        )));
    }
    if let Some(cpw) = node.as_constant_path_write_node() {
        let target = cpw.target();
        // A dynamic scope needs no hidden binding here -- a plain write reads
        // the scope exactly once -- but it must still be evaluated BEFORE the
        // right-hand side, which is ruby's order and the node's own.
        if let Some((parent, name)) = dynamic_const_scope(&target)? {
            let scope = lower_node(result, hir, &parent)?;
            let value = lower_node(result, hir, &cpw.value())?;
            return Ok(Some(hir.push(HirNode::DynConstWrite {
                scope,
                name,
                value,
            })));
        }
        let (scope, name) = constant_path_scope_and_name(&target)?;
        let value = lower_node(result, hir, &cpw.value())?;
        return Ok(Some(hir.push(HirNode::ConstWrite {
            scope: Some(scope),
            name,
            value,
        })));
    }
    if let Some(cp) = node.as_constant_path_node() {
        // `box::X`: an external access into the box -- the
        // ordinary bare-name lowering, wrapped in the box's scope.
        // A single segment lowers as a bare `ClassRef` (codegen's class-
        // or-constant rule under the box); deeper paths as the qualified
        // read they'd be inside the box.
        if let Some((bx, path)) = box_rooted_path(node) {
            let parsed = crate::constpath::ConstPath::parse(&path);
            let inner = match parsed.scope() {
                Some(scope) => hir.push(HirNode::QualifiedConstRead(
                    scope.to_string(),
                    parsed.base().to_string(),
                )),
                None => hir.push(HirNode::ClassRef(path.clone())),
            };
            return Ok(Some(hir.push(HirNode::BoxScope {
                box_id: bx,
                body: vec![inner],
            })));
        }
        // A DYNAMIC scope (`self.class::Reason`, `@rbconfig::CONFIG`): no
        // segment of the path's parent names a static owner, so ask whether
        // the WHOLE parent spells a constant path rather than just its outer
        // node -- `self::Readline::HISTORY` (irb's input-method.rb) has a
        // parent that is itself a path, and only its root is dynamic.
        // Evaluate the scope as a value and run the SCOPE OPERATOR's own
        // search on it. optparse's `self.class::Reason`.
        if let Some((parent, name)) = dynamic_const_scope(&cp)? {
            let scope = lower_node(result, hir, &parent)?;
            return Ok(Some(hir.push(HirNode::DynConstRead {
                scope,
                name,
                lenient: false,
            })));
        }
        let (scope, name) = constant_path_scope_and_name(&cp)?;
        return Ok(Some(hir.push(HirNode::QualifiedConstRead(scope, name))));
    }

    // A bare constant used as a VALUE -- currently only meaningful as a call
    // receiver (`ClassName.foo`); see `HirNode::ClassRef`'s docs. Falls
    // through generically via the ordinary `Call` receiver-lowering path
    // below, so no change is needed there.
    if let Some(c) = node.as_constant_read_node() {
        let name = String::from_utf8_lossy(c.name().as_slice()).into_owned();
        return Ok(Some(hir.push(HirNode::ClassRef(name))));
    }

    Ok(None)
}

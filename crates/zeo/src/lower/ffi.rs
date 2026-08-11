//! The FFI directive family (the real `ffi` gem's idioms): `extend
//! FFI::Library` recognition, `ffi_lib`/`typedef`/`enum`/`callback`/
//! `attach_function` lowering, the `class < FFI::Struct` `layout` ->
//! accessor-method synthesis, and the C type-name mapping shared by both.
//! Split out of `parse/mod.rs`.

use super::PResult;
use super::literals::assemble_i64;
use crate::hir::{Hir, HirNode, NodeId, Params, Visibility};
use ruby_prism::{Node, ParseResult};

/// `extend FFI::Library` -- the marker that turns a module into an FFI library
/// (the real `ffi` gem's idiom). Recognized syntactically so the `FFI::Library`
/// constant never has to resolve at runtime.
pub(crate) fn is_extend_ffi_library(node: &Node<'_>) -> bool {
    let Some(call) = node.as_call_node() else {
        return false;
    };
    if call.receiver().is_some() || call.name().as_slice() != b"extend" {
        return false;
    }
    let Some(args) = call.arguments() else {
        return false;
    };
    let mut it = args.arguments().iter();
    match (it.next(), it.next()) {
        (Some(arg), None) => const_path_string(&arg).as_deref() == Some("FFI::Library"),
        _ => false,
    }
}

/// `extend FFI::DataConverter` -- the class converts to/from a native FFI
/// type it names with `native_type`. Recognized so the CLASS NAME itself
/// works in later type positions (google-protobuf's `Internal::Arena` is
/// `native_type ::FFI::Type::POINTER`, then appears in `attach_function`
/// argument lists 104 corpus rows deep).
pub(crate) fn is_extend_ffi_data_converter(node: &Node<'_>) -> bool {
    let Some(call) = node.as_call_node() else {
        return false;
    };
    if call.receiver().is_some() || call.name().as_slice() != b"extend" {
        return false;
    }
    let Some(args) = call.arguments() else {
        return false;
    };
    let mut it = args.arguments().iter();
    match (it.next(), it.next()) {
        (Some(arg), None) => const_path_string(&arg).as_deref() == Some("FFI::DataConverter"),
        _ => false,
    }
}

/// The native type a `native_type <T>` statement declares, spelled as a
/// symbol (`native_type :pointer`) or an `FFI::Type::X` constant.
pub(crate) fn native_type_of(node: &Node<'_>) -> Option<crate::hir::FfiType> {
    let call = node.as_call_node()?;
    if call.receiver().is_some() || call.name().as_slice() != b"native_type" {
        return None;
    }
    let mut it = call.arguments()?.arguments().iter();
    let (arg, None) = (it.next()?, it.next()) else {
        return None;
    };
    if let Some(sym) = arg.as_symbol_node() {
        let empty = std::collections::HashMap::new();
        return ffi_type_of(&String::from_utf8_lossy(sym.unescaped()), &empty).ok();
    }
    let path = const_path_string(&arg)?;
    ffi_type_constant(path.trim_start_matches("::").strip_prefix("FFI::Type::")?)
}

/// The `FFI::Type::X` constants -- `zeo_abi::ffi::CScalar`'s table, which the
/// runtime's `FFI::Type` objects also answer to.
fn ffi_type_constant(leaf: &str) -> Option<crate::hir::FfiType> {
    zeo_abi::ffi::CScalar::from_type_constant(leaf).map(Into::into)
}

/// Flatten a constant reference (`FFI`, `FFI::Library`, `FFI::Library::LIBC`) to
/// its `::`-joined spelling, or `None` if it isn't a plain constant path.
fn const_path_string(node: &Node<'_>) -> Option<String> {
    if let Some(c) = node.as_constant_read_node() {
        return Some(String::from_utf8_lossy(c.name().as_slice()).into_owned());
    }
    let path = node.as_constant_path_node()?;
    let name = String::from_utf8_lossy(path.name()?.as_slice()).into_owned();
    match path.parent() {
        Some(parent) => Some(format!("{}::{name}", const_path_string(&parent)?)),
        None => Some(name),
    }
}

/// Lower one directive inside an FFI-library module. Returns `true` if it WAS an
/// FFI directive (`ffi_lib` / `attach_function`), `false` to fall through to the
/// ordinary class-body lowering.
pub(crate) fn lower_ffi_directive(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
    ffi_lib: &mut crate::hir::FfiLib,
    aliases: &mut std::collections::HashMap<String, crate::hir::FfiType>,
    out: &mut Vec<NodeId>,
) -> PResult<bool> {
    // Catch up on types declared since this body's snapshot: a struct or
    // typedef declared by a NESTED class body mid-module (sha3 nests its
    // state struct inside the library module, above the `attach_function`s
    // that pass it).
    for (k, v) in hir.inherited_ffi_types() {
        aliases.entry(k).or_insert(v);
    }
    // `SassTag = enum(:sass_boolean, :sass_number, ...)` -- the ANONYMOUS enum,
    // named by the constant it is assigned to rather than by a `:tag` argument.
    // sassc and google-protobuf both declare every one of their enums this way,
    // and then use the constant as a field/argument type. The gem's own
    // disambiguation: a leading symbol FOLLOWED BY an array is the NAMED form
    // (`Tag = enum :tag, [members]`), registered under the tag AND the
    // constant; any other shape reads every argument as a member.
    if let Some(write) = node.as_constant_write_node()
        && let Some(call) = write.value().as_call_node()
        && call.receiver().is_none()
        && call.name().as_slice() == b"enum"
    {
        let args: Vec<Node<'_>> = call
            .arguments()
            .map(|a| a.arguments().iter().collect())
            .unwrap_or_default();
        let name = String::from_utf8_lossy(write.name().as_slice()).into_owned();
        let (tag, members) = match (args.first(), args.get(1)) {
            (Some(t), Some(l)) if t.as_symbol_node().is_some() && l.as_array_node().is_some() => (
                Some(ffi_symbol_str(t)?),
                parse_enum_members(std::slice::from_ref(l), hir, out)?,
            ),
            _ => (None, parse_enum_members(&args, hir, out)?),
        };
        let ty = crate::hir::FfiType::Enum(members);
        if let Some(tag) = tag {
            hir.declare_ffi_type(&tag, &ty);
            aliases.insert(tag, ty.clone());
        }
        hir.declare_ffi_type(&name, &ty);
        aliases.insert(name, ty);
        // Consumed, like every other declaration here: the enum is a TYPE, and
        // zeo has no `FFI::Enum` object to bind the constant to. A program that
        // reads the constant at runtime gets a NameError -- loud, not wrong.
        return Ok(true);
    }
    let Some(call) = node.as_call_node() else {
        return Ok(false);
    };
    // `FFI.add_typedef(:uint32, :OM_uint32)` is `typedef` spelled on the FFI
    // module itself, same argument order. gssapi declares its whole C type
    // vocabulary that way, in a file above the structs that use it -- which the
    // program-wide table (`Hir::ffi_types`) is what makes reachable.
    let on_ffi_module = call
        .receiver()
        .and_then(|r| const_path_string(&r))
        .is_some_and(|p| p == "FFI" || p == "::FFI");
    if call.receiver().is_some() && !(on_ffi_module && call.name().as_slice() == b"add_typedef") {
        return Ok(false);
    }
    let args: Vec<Node<'_>> = call
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    match call.name().as_slice() {
        b"add_typedef" => {
            if args.len() != 2 {
                return Err(format!(
                    "FFI.add_typedef expects 2 arguments (existing_type, new_name), got {}",
                    args.len()
                )
                .into());
            }
            let existing = ffi_type_node(&args[0], aliases)?;
            let new_name = ffi_symbol_str(&args[1])?;
            hir.declare_ffi_type(&new_name, &existing);
            aliases.insert(new_name, existing);
            Ok(true)
        }
        b"ffi_lib" => {
            // `ffi_lib "m"` / `ffi_lib FFI::Library::LIBC` / a candidate list.
            // The most-recently declared library links every subsequent
            // `attach_function`.
            if !args.is_empty() {
                // `ffi_lib FFI::CURRENT_PROCESS`: the symbols come from the
                // already-linked image, which is exactly what a `lib` of
                // `None` emits -- an extern block with no `#[link]`.
                if names_current_process(&args) {
                    *ffi_lib = crate::hir::FfiLib::None;
                } else if let Some(lib) = ffi_lib_name(&args, hir) {
                    *ffi_lib = lib;
                } else {
                    // NOTHING folds (an ENV read, a local, a helper call):
                    // evaluate the candidate expressions when the class body
                    // EXECUTES -- `__zeo_ffi_lib` flattens the values and
                    // dlopens eagerly, so an unopenable library is CRuby's
                    // require-time `LoadError` at this very statement -- and
                    // every following `attach_function` resolves its symbol
                    // from the slot's handle.
                    let slot = hir.ffi_lib_slots;
                    hir.ffi_lib_slots += 1;
                    let slot_lit = hir.push(HirNode::IntegerLit(slot as i64));
                    let mut call_args = vec![crate::hir::ArrayElem::Single(slot_lit)];
                    // Each argument rides as a (splat?, expr) pair: the gem
                    // loads EVERY top-level argument as its own library (an
                    // Array value lists alternatives for one), so a splat
                    // must expand back into separate values -- codegen reads
                    // the flag and spreads the evaluated array.
                    for a in &args {
                        let (splatted, id) = match a.as_splat_node() {
                            Some(s) => {
                                let inner = s.expression().ok_or_else(|| {
                                    "ffi_lib can't forward a bare `*` splat (zeo limitation)"
                                        .to_string()
                                })?;
                                (1, super::lower_node(result, hir, &inner)?)
                            }
                            None => (0, super::lower_node(result, hir, a)?),
                        };
                        let flag = hir.push(HirNode::IntegerLit(splatted));
                        call_args.push(crate::hir::ArrayElem::Single(flag));
                        call_args.push(crate::hir::ArrayElem::Single(id));
                    }
                    out.push(hir.push(HirNode::Call {
                        receiver: None,
                        name: "__zeo_ffi_lib".to_string(),
                        args: call_args,
                        kwargs: Vec::new(),
                        block: None,
                        block_arg: None,
                        safe: false,
                    }));
                    *ffi_lib = crate::hir::FfiLib::Deferred { slot };
                }
            }
            Ok(true)
        }
        b"typedef" => {
            // `typedef :existing, :alias` -- register a type alias resolvable by
            // every subsequent `attach_function`. The gem requires the definition
            // precede its use, which source-order iteration gives us for free.
            if args.len() != 2 {
                return Err(format!(
                    "typedef expects 2 arguments (existing_type, new_name), got {}",
                    args.len()
                )
                .into());
            }
            let existing = ffi_type_node(&args[0], aliases)?;
            let new_name = ffi_symbol_str(&args[1])?;
            hir.declare_ffi_type(&new_name, &existing);
            aliases.insert(new_name, existing);
            Ok(true)
        }
        b"enum" => {
            // `enum :tag, [:sym, val, :sym, ...]` -- register `:tag` as an enum
            // type usable in a later type list. (A bare `enum [...]` statement,
            // whose members become module values with no type name at all, is
            // still a follow-on; the constant-assigned form is handled above.)
            let (tag, members) = match (args.first(), args.get(1)) {
                (Some(n), Some(l)) if n.as_symbol_node().is_some() => (
                    ffi_symbol_str(n)?,
                    parse_enum_members(std::slice::from_ref(l), hir, out)?,
                ),
                // A NAMELESS `enum [:a, :b]` statement registers no type
                // name, so nothing later can reference it in a type
                // position; its one effect -- symbol/int conversion for
                // arguments typed with THAT enum -- is unreachable without
                // a name. Validate the members and consume the statement.
                (Some(l), None) if l.as_array_node().is_some() => {
                    parse_enum_members(std::slice::from_ref(l), hir, out)?;
                    return Ok(true);
                }
                _ => {
                    return Err("enum expects `:tag, [members]` or `[members]`"
                        .to_string()
                        .into());
                }
            };
            let ty = crate::hir::FfiType::Enum(members);
            hir.declare_ffi_type(&tag, &ty);
            aliases.insert(tag, ty);
            Ok(true)
        }
        b"callback" => {
            // `callback :tag, [arg_types], ret_type` -- register `:tag` as a C
            // function-pointer type, carrying its full signature so a Ruby Proc
            // passed for a `:tag` argument can be marshaled into a libffi closure
            // (see codegen's `ffi_marshal_in`).
            let (tag, params, ret) = match (args.first(), args.get(1), args.get(2)) {
                (Some(t), Some(p), Some(r)) if t.as_symbol_node().is_some() => {
                    (ffi_symbol_str(t)?, p, r)
                }
                _ => {
                    return Err("callback expects `:tag, [arg_types], return_type`"
                        .to_string()
                        .into());
                }
            };
            let arg_types = ffi_type_array(params, aliases)?;
            let ret_ty = ffi_type_node(ret, aliases)?;
            if arg_types
                .iter()
                .chain(std::iter::once(&ret_ty))
                .any(|t| matches!(t, crate::hir::FfiType::Struct(_)))
            {
                return Err(
                    "a callback signature can't pass a struct BY VALUE (zeo limitation) -- \
                     use `.by_ref`"
                        .to_string()
                        .into(),
                );
            }
            let ty = crate::hir::FfiType::Callback(arg_types, Box::new(ret_ty));
            hir.declare_ffi_type(&tag, &ty);
            aliases.insert(tag, ty);
            Ok(true)
        }
        b"attach_function" => {
            out.push(lower_attach_function(
                result,
                hir,
                &args,
                ffi_lib.clone(),
                aliases,
            )?);
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// `[:ok, 0, :busy, 3, :error]` -> `[("ok",0),("busy",3),("error",4)]`. Members
/// are symbols, each optionally followed by an explicit integer value; an
/// omitted value auto-increments from the previous (starting at 0), exactly as
/// the `ffi` gem's `enum` does. A value may also be a constant this class body
/// already set to an integer (`ffi_const_int`).
fn parse_enum_members(
    args: &[Node<'_>],
    hir: &Hir,
    body_so_far: &[NodeId],
) -> PResult<Vec<(String, i64)>> {
    // The members are either one literal array or the argument list itself --
    // `enum :tag, [:a, :b]` and `enum(:a, :b)` both reach here.
    let unwrapped: Vec<Node<'_>>;
    let elems: &[Node<'_>] = match args {
        [one] if one.as_array_node().is_some() => {
            unwrapped = one
                .as_array_node()
                .expect("just matched")
                .elements()
                .iter()
                .collect();
            &unwrapped
        }
        _ => args,
    };
    if elems.is_empty() {
        return Err("enum expects at least one member".to_string().into());
    }
    let mut out: Vec<(String, i64)> = Vec::new();
    let mut next = 0i64;
    let mut i = 0;
    while i < elems.len() {
        let name = ffi_symbol_str(&elems[i])?;
        i += 1;
        let value = match elems.get(i).and_then(|n| ffi_const_int(n, hir, body_so_far)) {
            Some(v) => {
                i += 1;
                v
            }
            None => next,
        };
        out.push((name, value));
        next = value + 1;
    }
    Ok(out)
}

/// A compile-time INTEGER in an FFI declaration position (`0`, `(1 << 5)`,
/// `FFI::Type::LONG.size`, a constant this class body already set to one), or
/// `None` if the node isn't one (i.e. the next member symbol, or the list's
/// end). Enum member values and inline-array counts both fold through here:
/// an FFI declaration's integers decide marshaling tables and field offsets,
/// so they are needed at LOWERING time -- this is deliberately not a general
/// constant folder.
fn ffi_const_int(node: &Node<'_>, hir: &Hir, body_so_far: &[NodeId]) -> Option<i64> {
    if let Some(n) = body_const_int(hir, body_so_far, node) {
        return Some(n);
    }
    enum_int_literal(node, hir, body_so_far)
}

fn enum_int_literal(node: &Node<'_>, hir: &Hir, body_so_far: &[NodeId]) -> Option<i64> {
    if let Some(int) = node.as_integer_node() {
        let value = int.value();
        let (negative, digits) = value.to_u32_digits();
        return assemble_i64(negative, digits);
    }
    // `(1 << 0)` -- flag enums are written as shifts and ors far more often
    // than as the numbers they come to, and the value is a compile-time
    // constant either way. gir_ffi's `enum :IRepositoryLoadFlags, [:LAZY, (1 <<
    // 0)]` is the case. Folded here rather than left to a general constant
    // folder because an enum member's value is needed at LOWERING time: it is
    // what the generated marshaling tables are built from.
    if let Some(paren) = node.as_parentheses_node()
        && let Some(stmts) = paren.body()
        && let Some(stmts) = stmts.as_statements_node()
    {
        let only: Vec<Node<'_>> = stmts.body().iter().collect();
        if let [inner] = only.as_slice() {
            return ffi_const_int(inner, hir, body_so_far);
        }
        return None;
    }
    let call = node.as_call_node()?;
    let recv = call.receiver()?;
    let args: Vec<Node<'_>> = call
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    // `::FFI::Type::LONG.size` -- a scalar's byte width, a constant of the
    // target. dcu-typhoeus sizes its fd_set inline array with it.
    if call.name().as_slice() == b"size"
        && args.is_empty()
        && let Some(path) = const_path_string(&recv)
        && let Some(leaf) = path.trim_start_matches("::").strip_prefix("FFI::Type::")
        && let Some(s) = zeo_abi::ffi::CScalar::from_type_constant(leaf)
        && !matches!(s, zeo_abi::ffi::CScalar::Void)
    {
        return Some(s.size() as i64);
    }
    let lhs = ffi_const_int(&recv, hir, body_so_far)?;
    match (call.name().as_slice(), args.as_slice()) {
        (b"-@", []) => lhs.checked_neg(),
        (b"~", []) => Some(!lhs),
        (op, [rhs]) => {
            let rhs = ffi_const_int(rhs, hir, body_so_far)?;
            match op {
                b"<<" => u32::try_from(rhs).ok().and_then(|s| lhs.checked_shl(s)),
                b">>" => u32::try_from(rhs).ok().and_then(|s| lhs.checked_shr(s)),
                b"|" => Some(lhs | rhs),
                b"&" => Some(lhs & rhs),
                b"^" => Some(lhs ^ rhs),
                b"+" => lhs.checked_add(rhs),
                b"-" => lhs.checked_sub(rhs),
                b"*" => lhs.checked_mul(rhs),
                b"/" if rhs != 0 => lhs.checked_div(rhs),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Recognize an FFI `layout :name, :type, :name, :type, ...` directive inside a
/// `class < FFI::Struct` body and return its `(field, type)` pairs, or `None`
/// if `node` isn't a `layout` call.
pub(crate) fn as_ffi_layout(
    node: &Node<'_>,
    aliases: &std::collections::HashMap<String, crate::hir::FfiType>,
    hir: &Hir,
    body_so_far: &[NodeId],
) -> PResult<Option<Vec<(String, crate::hir::FfiType)>>> {
    let Some(call) = node.as_call_node() else {
        return Ok(None);
    };
    if call.receiver().is_some() || call.name().as_slice() != b"layout" {
        return Ok(None);
    }
    let args: Vec<Node<'_>> = call
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    let field =
        |name_node: &Node<'_>, ty_node: &Node<'_>| -> PResult<(String, crate::hir::FfiType)> {
            let name = ffi_symbol_str(name_node)?;
            let ty = match layout_array_type(ty_node, aliases, hir, body_so_far)? {
                Some(t) => t,
                None => ffi_type_node(ty_node, aliases)?,
            };
            Ok((name, ty))
        };
    // The gem's documented alternative spellings: one hash instead of a flat
    // pair list -- `layout(magic: :uint32)` (a trailing keyword hash) and
    // `layout({ :dwId => :uint })` (a braced one).
    if args.len() == 1
        && let Some(pairs) = hash_pairs(&args[0])
    {
        let mut fields = Vec::new();
        for (k, v) in &pairs {
            fields.push(field(k, v)?);
        }
        return Ok(Some(fields));
    }
    if args.is_empty() || !args.len().is_multiple_of(2) {
        return Err("FFI::Struct `layout` expects `:name, :type` pairs"
            .to_string()
            .into());
    }
    let mut fields = Vec::new();
    let mut i = 0;
    while i < args.len() {
        fields.push(field(&args[i], &args[i + 1])?);
        i += 2;
    }
    Ok(Some(fields))
}

/// A hash literal's `(key, value)` node pairs -- braced or keyword form. Any
/// non-pair element (a `**splat`) declines the whole hash.
fn hash_pairs<'a>(node: &Node<'a>) -> Option<Vec<(Node<'a>, Node<'a>)>> {
    let elements: Vec<Node<'a>> = if let Some(h) = node.as_hash_node() {
        h.elements().iter().collect()
    } else if let Some(h) = node.as_keyword_hash_node() {
        h.elements().iter().collect()
    } else {
        return None;
    };
    elements
        .into_iter()
        .map(|e| e.as_assoc_node().map(|a| (a.key(), a.value())))
        .collect()
}

/// `[:uint8, 384]` in a layout TYPE position -- an inline array of 384 bytes
/// stored in place. `None` when the node isn't an array literal at all.
///
/// The element count has to be DECIDABLE: it fixes every following field's
/// offset, so a count zeo cannot read would be a wrong struct rather than a
/// slower one. An integer literal, or a constant this class body already
/// assigned an integer -- sys-filesystem's `UUID_NODE_LEN = 6` two lines above
/// its `layout(...)` is the shape, and C bindings spell array widths that way
/// far more often than not.
fn layout_array_type(
    node: &Node<'_>,
    aliases: &std::collections::HashMap<String, crate::hir::FfiType>,
    hir: &Hir,
    body_so_far: &[NodeId],
) -> PResult<Option<crate::hir::FfiType>> {
    let Some(array) = node.as_array_node() else {
        return Ok(None);
    };
    let elems: Vec<Node<'_>> = array.elements().iter().collect();
    if elems.len() != 2 {
        return Err(
            "an inline array field is written `[element_type, count]` (zeo limitation)"
                .to_string()
                .into(),
        );
    }
    let elem = ffi_type_node(&elems[0], aliases)?;
    let count = ffi_const_int(&elems[1], hir, body_so_far)
        .ok_or_else(|| {
            "an inline array field's element COUNT must be an integer literal, or a constant this \
             class body already set to one -- it decides where every following field starts (zeo \
             limitation)"
                .to_string()
        })?;
    if count < 0 {
        return Err("an inline array field's element count can't be negative"
            .to_string()
            .into());
    }
    Ok(Some(crate::hir::FfiType::Array(
        Box::new(elem),
        count as usize,
    )))
}

/// The integer a bare constant names, read off the `ConstWrite` this class body
/// already lowered for it. Scoped to the body on purpose: a layout's array
/// width is written beside the layout, and reaching further would mean deciding
/// a name against a scope chain that is still being built.
fn body_const_int(hir: &Hir, body_so_far: &[NodeId], node: &Node<'_>) -> Option<i64> {
    let wanted = node.as_constant_read_node()?;
    let wanted = String::from_utf8_lossy(wanted.name().as_slice()).into_owned();
    body_so_far.iter().rev().find_map(|&id| match &hir[id] {
        HirNode::ConstWrite { name, value, .. } if *name == wanted => match hir[*value] {
            HirNode::IntegerLit(n) => Some(n),
            _ => None,
        },
        _ => None,
    })
}

/// The `FFI::MemoryPointer` accessor pair and C layout `(size, align)` for a
/// struct field type. Structs hold scalar/pointer fields; a `:string`/`:bool`/
/// nested-struct field is a clean, greppable rejection (follow-on).
fn ffi_field_accessor(ty: &crate::hir::FfiType) -> PResult<(String, String, usize, usize)> {
    use crate::hir::FfiType::*;
    // An inline array occupies `count` elements IN PLACE, and aligns to one
    // element -- so it is the field that decides where the next one starts.
    // The getter/putter named here are the ELEMENT's, which is what the
    // synthesized proxy indexes with.
    if let Array(elem, count) = ty {
        if matches!(**elem, Struct(_) | Callback(..)) {
            return Err(
                "an inline array of structs or callbacks isn't supported yet (zeo limitation)"
                    .to_string()
                    .into(),
            );
        }
        let (get, put, esize, ealign) = ffi_field_accessor(elem)?;
        return Ok((get, put, esize * count, ealign));
    }
    // A nested struct stored BY VALUE has no scalar accessor pair -- the
    // synthesized reader hands back the struct class VIEWING the field's
    // bytes in place, and the writer copies bytes -- see `Conv::Struct`.
    if let Struct(l) = ty {
        if l.class_path.is_empty() {
            return Err(
                "a nested struct field needs a NAMED struct class (zeo limitation)"
                    .to_string()
                    .into(),
            );
        }
        return Ok((String::new(), String::new(), l.size, l.align));
    }
    let (get, put) = match ty {
        Int(w) => (format!("get_int{w}"), format!("put_int{w}")),
        Uint(w) => (format!("get_uint{w}"), format!("put_uint{w}")),
        Float(w) => (format!("get_float{w}"), format!("put_float{w}")),
        // An enum field is a C `int` in memory, a bool a one-byte `_Bool`.
        // Neither reads back as the number it stores; the generated accessor
        // converts -- see `synthesize_ffi_struct`.
        Enum(_) => ("get_int32".into(), "put_int32".into()),
        Bool => ("get_int8".into(), "put_int8".into()),
        // A `:string` field is a `char *`: read through the pointer, and NOT
        // writable -- CRuby's ffi raises `Cannot set :string fields`, because
        // storing one would need somewhere to keep the bytes alive. A
        // callback field is a C function pointer in memory; the generated
        // accessor wraps/unwraps `FFI::Function` -- see `Conv::Callback`.
        Str | Pointer | Callback(..) => ("get_pointer".into(), "put_pointer".into()),
        other => {
            return Err(format!(
                "FFI::Struct field type `{other:?}` isn't supported yet"
            )
            .into());
        }
    };
    // Width and alignment come from the one shared table -- the same widths
    // codegen's `#[repr(C)]` mirror asserts against.
    let s = ty.c_scalar().expect("the unsupported arms returned above");
    Ok((get, put, s.size(), s.align()))
}

/// A type's spelling in SYNTHESIZED ruby source (an `FFI::Function.new`
/// signature for a callback field): the canonical scalar keyword, with
/// `FFI::Type::VOID` for void -- the keyword table deliberately rejects
/// `:void` in value positions, and the constant resolves to the same kind.
fn ruby_ffi_type_src(ty: &crate::hir::FfiType) -> PResult<String> {
    use crate::hir::FfiType::*;
    Ok(match ty {
        Void => "::FFI::Type::VOID".to_string(),
        Enum(_) => ":int32".to_string(),
        Callback(..) => ":pointer".to_string(),
        Struct(_) | Array(..) => {
            return Err(
                "a callback signature can't pass a struct BY VALUE (zeo limitation) -- \
                 use `.by_ref`"
                    .to_string()
                    .into(),
            );
        }
        scalar => format!(
            ":{}",
            scalar
                .c_scalar()
                .expect("the aggregate arms returned above")
                .keyword()
        ),
    })
}

/// Synthesize the Ruby methods for a `class < FFI::Struct` from its `layout`:
/// `[]`/`[]=` read/write each field at its computed C offset over an owned
/// `FFI::MemoryPointer` ivar, plus `pointer`/`to_ptr`, `size`, `offset_of`, and
/// `members`. Offsets follow C alignment (each field aligned to its own size;
/// total rounded to the max field alignment), matching `ffi 1.17.4` and the C
/// ABI. Returned as source for `parse_and_lower_into`.
/// The two classes an inline array field reads back as, as ruby source.
///
/// Emitted at ABSOLUTE scope (`module ::FFI`) from inside the struct body that
/// first needs them, and only once per program -- redefining them per struct
/// would print a method-redefined warning for every struct after the first.
/// The names are observable (`s[:bytes].class`), so they are the gem's, and so
/// is the split: `CharArray` is the 8-bit one, and the only one with `to_s`.
///
/// Written against `send` on the pointer rather than a per-element-type class,
/// so one pair of classes serves every element width.
const FFI_INLINE_ARRAY_CLASSES: &str = r#"
module ::FFI
  class Struct
    class InlineArray
      include ::Enumerable
      def initialize(__p, __off, __n, __get, __put, __esize)
        @__p, @__off, @__n, @__get, @__put, @__esize = __p, __off, __n, __get, __put, __esize
      end
      def size
        @__n
      end
      def [](__i)
        @__p.send(@__get, @__off + __i * @__esize)
      end
      def []=(__i, __v)
        @__p.send(@__put, @__off + __i * @__esize, __v)
      end
      def each
        __i = 0
        while __i < @__n
          yield self[__i]
          __i += 1
        end
        self
      end
      def to_a
        ::Array.new(@__n) { |__i| self[__i] }
      end
      def to_ptr
        @__p
      end
    end
  end
  class StructLayout
    class CharArray < ::FFI::Struct::InlineArray
      def to_s
        __out = []
        __i = 0
        while __i < @__n
          __b = self[__i]
          break if __b == 0
          __out << (__b & 0xff)
          __i += 1
        end
        __out.pack("C*")
      end
      alias to_str to_s
    end
  end
end
"#;

/// Whether `fields` needs the inline-array proxy classes emitted with them.
pub(crate) fn needs_inline_array_classes(fields: &[(String, crate::hir::FfiType)]) -> bool {
    fields
        .iter()
        .any(|(_, t)| matches!(t, crate::hir::FfiType::Array(..)))
}

/// The one place field offsets, total size and alignment are computed --
/// consumed by the accessor synthesis below AND recorded as
/// `Hir::ffi_struct_layouts` for by-value passing, so the two views of the
/// same struct cannot disagree.
pub(crate) fn ffi_struct_layout(
    class_path: &str,
    fields: &[(String, crate::hir::FfiType)],
    union: bool,
) -> PResult<crate::hir::FfiStructLayout> {
    let round_up = |n: usize, a: usize| -> usize { n.div_ceil(a) * a };
    let mut offset = 0usize;
    let mut max_align = 1usize;
    // A union's members all start at offset 0 and it is as wide as its
    // widest member.
    let mut widest = 0usize;
    let mut placed = Vec::new();
    for (name, ty) in fields {
        let (_, _, size, align) = ffi_field_accessor(ty)?;
        let off = if union { 0 } else { round_up(offset, align) };
        placed.push((name.clone(), ty.clone(), off));
        offset = off + size;
        widest = widest.max(size);
        max_align = max_align.max(align);
    }
    Ok(crate::hir::FfiStructLayout {
        class_path: class_path.to_string(),
        fields: placed,
        size: round_up(if union { widest } else { offset }, max_align),
        align: max_align,
        union,
    })
}

pub(crate) fn synthesize_ffi_struct(
    layout: &crate::hir::FfiStructLayout,
    with_inline_array_classes: bool,
) -> PResult<String> {
    // How a field's stored bytes become a ruby value and back. Most fields are
    // the number itself.
    enum Conv {
        Plain,
        Enum(Vec<(String, i64)>),
        Bool,
        Str,
        /// `(class, element count, element size)` -- the proxy the field reads
        /// back as. `FFI::StructLayout::CharArray` for an 8-bit element (it is
        /// the one that also answers `to_s`), `FFI::Struct::InlineArray`
        /// otherwise, matching the gem.
        Array(&'static str, usize, usize),
        /// A nested struct stored BY VALUE: `(class path, byte size)`.
        /// Reading yields the class VIEWING the field's bytes in place (a
        /// mutation through the view mutates the parent -- oracle-verified);
        /// writing copies the value's bytes over the field, both exactly as
        /// the gem does.
        Struct(String, usize),
        /// A C function-pointer field: `(ruby arg-type list, ruby return
        /// type)` as source text. Reading wraps the stored address in an
        /// `FFI::Function` (the gem's read class); writing accepts a
        /// pointer/Function as-is or marshals a callable into a Function --
        /// KEPT in an ivar so the closure outlives the write, which is
        /// sturdier than the gem's own keep-it-alive-yourself contract.
        Callback(String, String),
    }
    // (field, getter, putter, offset, conversion) -- offsets come off the
    // recorded layout, whose walk (`ffi_struct_layout`) is the ONE place they
    // are computed.
    let mut placed: Vec<(String, String, String, usize, Conv)> = Vec::new();
    for (name, ty, off) in &layout.fields {
        let (getter, putter, _, _) = ffi_field_accessor(ty)?;
        let conv = match ty {
            crate::hir::FfiType::Enum(m) => Conv::Enum(m.clone()),
            crate::hir::FfiType::Bool => Conv::Bool,
            crate::hir::FfiType::Str => Conv::Str,
            crate::hir::FfiType::Array(elem, count) => {
                let (_, _, esize, _) = ffi_field_accessor(elem)?;
                let class = if esize == 1 {
                    "FFI::StructLayout::CharArray"
                } else {
                    "FFI::Struct::InlineArray"
                };
                Conv::Array(class, *count, esize)
            }
            crate::hir::FfiType::Struct(l) => Conv::Struct(l.class_path.clone(), l.size),
            crate::hir::FfiType::Callback(cb_args, cb_ret) => {
                let args: Vec<String> = cb_args
                    .iter()
                    .map(ruby_ffi_type_src)
                    .collect::<PResult<_>>()?;
                Conv::Callback(args.join(", "), ruby_ffi_type_src(cb_ret)?)
            }
            _ => Conv::Plain,
        };
        placed.push((name.clone(), getter, putter, *off, conv));
    }
    let total = layout.size;

    // An enum field reads back as its member SYMBOL and accepts either a symbol
    // or the raw integer, which is `Enum#from_native`/`#to_native`. A value with
    // no member keeps its number, exactly as the gem's do.
    let read_arms: String = placed
        .iter()
        .map(|(name, getter, putter, off, conv)| {
            let read = format!("@__ffi_ptr.{getter}({off})");
            let read = match conv {
                Conv::Plain => read,
                Conv::Bool => format!("{read} != 0"),
                Conv::Str => format!("((__p = {read}).null? ? nil : __p.read_string)"),
                // A proxy OVER the struct's own memory, not a copy: writing
                // through it writes the struct, which is what the gem does.
                Conv::Array(class, count, esize) => format!(
                    "{class}.new(@__ffi_ptr, {off}, {count}, :{getter}, :{putter}, {esize})"
                ),
                // The nested class VIEWING the field's bytes in place -- the
                // synthesized `initialize` takes the pointer as-is, so every
                // inner accessor indexes from parent + offset.
                Conv::Struct(class, _) => format!("{class}.new(@__ffi_ptr + {off})"),
                Conv::Callback(args, ret) => format!(
                    "::FFI::Function.new({ret}, [{args}], @__ffi_ptr.get_pointer({off}))"
                ),
                Conv::Enum(m) => {
                    let table = m
                        .iter()
                        .map(|(n, v)| format!("{v} => :{n}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{{{table}}}.fetch({read}) {{ |__v| __v }}")
                }
            };
            format!("        when :{name} then {read}\n")
        })
        .collect();
    let write_arms: String = placed
        .iter()
        .map(|(name, _, putter, off, conv)| {
            // A `:string` field has nowhere to keep the bytes alive, so ruby
            // refuses the write rather than storing a dangling pointer.
            if matches!(conv, Conv::Str) {
                return format!(
                    "        when :{name} then raise ArgumentError, \"Cannot set :string fields\"\n"
                );
            }
            // Ruby refuses a whole-array assignment too -- the proxy's own
            // `[]=` is how an inline array is written.
            if matches!(conv, Conv::Array(..)) {
                return format!(
                    "        when :{name} then raise NotImplementedError, \"cannot set array field\"\n"
                );
            }
            // A nested struct write COPIES the value's bytes over the field
            // (oracle-verified memcpy semantics).
            if let Conv::Struct(_, size) = conv {
                return format!(
                    "        when :{name} then @__ffi_ptr.put_bytes({off}, \
                     __ffi_value.to_ptr.get_bytes(0, {size}))\n"
                );
            }
            // A callback write stores a code pointer: a pointer/Function
            // as-is, `nil` as NULL, and any other callable marshaled into an
            // `FFI::Function` -- kept in an ivar so the closure stays alive
            // as long as the struct.
            if let Conv::Callback(args, ret) = conv {
                return format!(
                    "        when :{name} then begin\n           __zeo_v = __ffi_value\n           \
                     unless __zeo_v.nil? || __zeo_v.is_a?(::FFI::Pointer)\n             \
                     __zeo_v = ::FFI::Function.new({ret}, [{args}], __zeo_v)\n             \
                     (@__zeo_cb_keep ||= {{}})[:{name}] = __zeo_v\n           end\n           \
                     @__ffi_ptr.put_pointer({off}, __zeo_v)\n         end\n"
                );
            }
            let value = match conv {
                Conv::Plain => "__ffi_value".to_string(),
                Conv::Bool => "(__ffi_value ? 1 : 0)".to_string(),
                Conv::Str | Conv::Array(..) | Conv::Struct(..) | Conv::Callback(..) => {
                    unreachable!("returned above")
                }
                Conv::Enum(m) => {
                    let table = m
                        .iter()
                        .map(|(n, v)| format!("{n}: {v}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{{{table}}}.fetch(__ffi_value, __ffi_value)")
                }
            };
            format!("        when :{name} then @__ffi_ptr.{putter}({off}, {value})\n")
        })
        .collect();
    let offset_arms: String = placed
        .iter()
        .map(|(name, _, _, off, _)| format!("        when :{name} then {off}\n"))
        .collect();
    let members: String = placed
        .iter()
        .map(|(name, ..)| format!(":{name}"))
        .collect::<Vec<_>>()
        .join(", ");

    let inline_array_classes = if with_inline_array_classes {
        FFI_INLINE_ARRAY_CLASSES
    } else {
        ""
    };
    Ok(format!(
        r#"{inline_array_classes}
def initialize(__ffi_ptr = nil)
  @__ffi_ptr = __ffi_ptr || FFI::MemoryPointer.new({total})
end
def [](__ffi_field)
  case __ffi_field
{read_arms}      else raise ArgumentError, "no such struct field #{{__ffi_field.inspect}}"
  end
end
def []=(__ffi_field, __ffi_value)
  case __ffi_field
{write_arms}      else raise ArgumentError, "no such struct field #{{__ffi_field.inspect}}"
  end
  __ffi_value
end
def pointer
  @__ffi_ptr
end
def to_ptr
  @__ffi_ptr
end
def self.size
  {total}
end
def self.offset_of(__ffi_field)
  case __ffi_field
{offset_arms}      else raise ArgumentError, "no such struct field #{{__ffi_field.inspect}}"
  end
end
def self.members
  [{members}]
end
"#
    ))
}

/// The library name for `#[link(name = ..)]` from a `ffi_lib` ARGUMENT LIST.
///
/// `ffi_lib` takes CANDIDATES -- several arguments, or one array of them -- and
/// ruby loads the first that dlopens. zeo has to name one library at compile
/// time, so it takes the first candidate it can decide. That is the bare name
/// gems write first (`ffi_lib ["sodium", "libsodium.so.18", "libsodium.so.23"]`
/// -- rbnacl), the later entries being versioned sonames of the same library.
///
/// A candidate it cannot decide is SKIPPED rather than refused: libusb leads
/// with two locals holding bundled paths and then names the system library.
/// A list where nothing at all is decidable answers `None`, and the caller
/// DEFERS the whole statement to runtime evaluation (`FfiLib::Deferred`) --
/// and if a folded name has no library to link against, the build says so,
/// loudly.
fn ffi_lib_name(args: &[Node<'_>], hir: &Hir) -> Option<crate::hir::FfiLib> {
    // Every candidate this can NAME, in the gem's try-in-order semantics. An
    // unfoldable candidate (a local, an ENV read, a helper call) is SKIPPED,
    // exactly as before: only an all-unfoldable list errors.
    let mut folded: Vec<String> = Vec::new();
    for a in args {
        match a.as_array_node() {
            Some(arr) => {
                for e in arr.elements().iter() {
                    if let Some(s) = fold_lib_string(&e, hir) {
                        folded.push(s);
                    }
                }
            }
            None => {
                if let Some(s) = fold_lib_string(a, hir) {
                    folded.push(s);
                }
            }
        }
    }
    let first = folded.first()?;
    // A plain name links at BUILD time -- the zero-overhead tier, and what
    // every `ffi_lib "m"` always got. A PATH only a running process can
    // resolve (a bundled `.so` beside the gem's own files) goes to the
    // runtime dlopen tier, which is when and where CRuby's ffi gem opens
    // every library.
    if !first.contains('/') {
        return Some(crate::hir::FfiLib::Static(strip_lib_name(first)));
    }
    Some(crate::hir::FfiLib::Runtime(folded))
}

/// A candidate's compile-time STRING value, or `None` when it is only
/// decidable at run time (a local, an ENV read, a helper call).
///
/// NOT special-cased: `ffi_lib FFI.library_name("vips", 42)` (ruby-vips).
/// `library_name` is not an `ffi` API -- ruby-vips reopens `module FFI` and
/// defines it -- so folding it by name would hard-code one gem's helper into
/// the compiler and miscompile the next gem to pick the same name.
fn fold_lib_string(node: &Node<'_>, hir: &Hir) -> Option<String> {
    if let Some(s) = node.as_string_node() {
        return Some(String::from_utf8_lossy(s.unescaped()).into_owned());
    }
    // `ffi_lib :kernel32, :user32` -- windows gems name their DLLs as symbols.
    if let Some(s) = node.as_symbol_node() {
        return Some(String::from_utf8_lossy(s.unescaped()).into_owned());
    }
    // `"#{__dir__}/../ext/libfoo.so"` -- parts fold independently.
    if let Some(interp) = node.as_interpolated_string_node() {
        let mut out = String::new();
        for part in interp.parts().iter() {
            if let Some(s) = part.as_string_node() {
                out.push_str(&String::from_utf8_lossy(s.unescaped()));
            } else if let Some(embedded) = part.as_embedded_statements_node() {
                let stmts: Vec<Node<'_>> = embedded
                    .statements()
                    .map(|s| s.body().iter().collect())
                    .unwrap_or_default();
                let [only] = stmts.as_slice() else {
                    return None;
                };
                out.push_str(&fold_lib_string(only, hir)?);
            } else {
                return None;
            }
        }
        return Some(out);
    }
    if let Some(call) = node.as_call_node() {
        let name = call.name().as_slice();
        let args: Vec<Node<'_>> = call
            .arguments()
            .map(|a| a.arguments().iter().collect())
            .unwrap_or_default();
        // The requiring file's directory -- how a gem roots its bundled
        // native half.
        if name == b"__dir__" && call.receiver().is_none() && args.is_empty() {
            return Some(hir.lowering_dir.as_ref()?.display().to_string());
        }
        if name == b"+"
            && let Some(recv) = call.receiver()
            && let [rhs] = args.as_slice()
        {
            return Some(format!(
                "{}{}",
                fold_lib_string(&recv, hir)?,
                fold_lib_string(rhs, hir)?
            ));
        }
        let on_file = call
            .receiver()
            .and_then(|r| const_path_string(&r))
            .is_some_and(|p| p.trim_start_matches("::") == "File");
        if on_file {
            match (name, args.as_slice()) {
                (b"join", parts) if !parts.is_empty() => {
                    let folded: Option<Vec<String>> =
                        parts.iter().map(|p| fold_lib_string(p, hir)).collect();
                    return Some(folded?.join("/"));
                }
                (b"dirname", [p]) => {
                    let s = fold_lib_string(p, hir)?;
                    let parent = std::path::Path::new(&s).parent()?;
                    return Some(parent.display().to_string());
                }
                (b"expand_path", [p]) => {
                    return Some(lexical_join(
                        &hir.lowering_dir.clone()?,
                        &fold_lib_string(p, hir)?,
                    ));
                }
                (b"expand_path", [p, base]) => {
                    let base = fold_lib_string(base, hir)?;
                    return Some(lexical_join(
                        std::path::Path::new(&base),
                        &fold_lib_string(p, hir)?,
                    ));
                }
                _ => return None,
            }
        }
        return None;
    }
    // `FFI::Library::LIBC` names the platform C library (`c`, which resolves
    // to libSystem on macOS). `FFI::Platform::LIBC` is the same constant by
    // its other path, and either may be written `::`-anchored.
    let path = const_path_string(node)?;
    match path.trim_start_matches("::") {
        "FFI::Library::LIBC" | "FFI::Platform::LIBC" => Some("c".to_string()),
        _ => None,
    }
}

/// `base` joined with `rel`, `.`/`..` collapsed LEXICALLY -- the same rule as
/// `File.expand_path`, which never consults the filesystem.
fn lexical_join(base: &std::path::Path, rel: &str) -> String {
    use std::path::Component;
    let mut out = std::path::PathBuf::new();
    for component in base.join(rel).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out.display().to_string()
}

/// Whether any `ffi_lib` argument names the process itself.
fn names_current_process(args: &[Node<'_>]) -> bool {
    let is_marker = |n: &Node<'_>| {
        const_path_string(n).is_some_and(|p| {
            matches!(
                p.trim_start_matches("::"),
                "FFI::CURRENT_PROCESS" | "FFI::USE_THIS_PROCESS_AS_LIBRARY"
            )
        })
    };
    args.iter().any(|a| match a.as_array_node() {
        Some(arr) => arr.elements().iter().any(|e| is_marker(&e)),
        None => is_marker(a),
    })
}

/// `libm.so.6` / `libssl.dylib` / `m` -> the bare rustc link name (`m`/`ssl`).
fn strip_lib_name(raw: &str) -> String {
    let base = raw.rsplit('/').next().unwrap_or(raw);
    let base = base.split(['.']).next().unwrap_or(base);
    base.strip_prefix("lib").unwrap_or(base).to_string()
}

/// `attach_function :abs, [:int], :int` (plain) or `attach_function :my_len,
/// :strlen, [:string], :ulong` (the 4-arg rename form) -> a synthesized class
/// method whose body is a `HirNode::Ffi` over the C symbol.
fn lower_attach_function(
    result: &ParseResult,
    hir: &mut Hir,
    args: &[Node<'_>],
    lib: crate::hir::FfiLib,
    aliases: &std::collections::HashMap<String, crate::hir::FfiType>,
) -> PResult<NodeId> {
    let _ = result;
    // `attach_function(name, func = name, args, returns, options = {})`. The
    // options are ruby's own trailing hash, so prism hands them over as one
    // more element of the argument list -- 3/4 arguments plus an optional one.
    let (positional, options) = match args.last().filter(|a| is_options_hash(a)) {
        Some(opts) => (&args[..args.len() - 1], Some(opts)),
        None => (args, None),
    };
    let (ruby_name, c_symbol, types_node, ret_node) = match positional.len() {
        3 => {
            let name = ffi_symbol_str(&positional[0])?;
            (name.clone(), name, &positional[1], &positional[2])
        }
        4 => (
            ffi_symbol_str(&positional[0])?,
            ffi_symbol_str(&positional[1])?,
            &positional[2],
            &positional[3],
        ),
        n => {
            return Err(format!(
                "attach_function expects 3 or 4 arguments (name, [args], ret), got {n}"
            )
            .into());
        }
    };
    let (arg_types, variadic) = ffi_arg_types(types_node, aliases)?;
    let ret = ffi_type_node(ret_node, aliases)?;
    let blocking = match options {
        Some(opts) => attach_function_options(opts)?,
        None => false,
    };
    // By-value structs ride the fixed `extern "C"` tier, where rustc owns the
    // ABI. The three positions that tier cannot express are clean rejections
    // at the declaration -- never a wrong call.
    let passes_struct = arg_types
        .iter()
        .any(|t| matches!(t, crate::hir::FfiType::Struct(_)));
    if matches!(ret, crate::hir::FfiType::Struct(_)) {
        return Err(
            "returning an FFI struct BY VALUE isn't supported yet (zeo limitation) -- return \
             `.by_ref` (a pointer) and wrap it"
                .to_string()
                .into(),
        );
    }
    if passes_struct && variadic {
        return Err(
            "a variadic `attach_function` can't pass a struct BY VALUE (zeo limitation) -- \
             use `.by_ref`"
                .to_string()
                .into(),
        );
    }
    if passes_struct
        && matches!(
            lib,
            crate::hir::FfiLib::Runtime(_) | crate::hir::FfiLib::Deferred { .. }
        )
    {
        return Err(
            "a runtime-resolved `ffi_lib` can't pass a struct BY VALUE (zeo limitation) -- \
             name the library statically or use `.by_ref`"
                .to_string()
                .into(),
        );
    }
    // A `blocking: true` call releases the GVL, and the C function may not call
    // back into ruby while it is released. ffi says the same; here it would be
    // a callback trampolining into a Proc with no GVL held.
    if blocking
        && arg_types
            .iter()
            .any(|t| matches!(t, crate::hir::FfiType::Callback(..)))
    {
        return Err(
            "attach_function `blocking: true` can't be combined with a callback argument \
             (the callback would re-enter ruby with the GVL released)"
                .to_string()
                .into(),
        );
    }

    // The wrapper's params: one required positional per FIXED C argument, named
    // so a `LocalRead` in the `Ffi` body reaches it; a variadic function also
    // gets a `*__ffi_rest` that collects the trailing (type, value) pairs.
    let param_names: Vec<String> = (0..arg_types.len())
        .map(|i| format!("__ffi_a{i}"))
        .collect();
    let call_args: Vec<(NodeId, crate::hir::FfiType)> = param_names
        .iter()
        .zip(arg_types)
        .map(|(name, ty)| (hir.push(HirNode::LocalRead(name.clone())), ty))
        .collect();
    let variadic_read = variadic.then(|| hir.push(HirNode::LocalRead("__ffi_rest".to_string())));
    let body = vec![hir.push(HirNode::Ffi(crate::hir::FfiCall {
        symbol: c_symbol,
        lib,
        args: call_args,
        ret,
        variadic: variadic_read,
        blocking,
    }))];
    let params = Params {
        required: param_names,
        rest: variadic.then(|| Some("__ffi_rest".to_string())),
        ..Default::default()
    };
    Ok(hir.push(HirNode::DefMethod {
        name: ruby_name,
        params,
        body,
        is_class_method: true,
        visibility: Visibility::Public,
        is_def: true,
    }))
}

/// Whether a trailing `attach_function` argument is its options hash rather
/// than a type. Only the hash forms qualify, so a 5th POSITIONAL argument is
/// still the arity error it was.
fn is_options_hash(node: &Node<'_>) -> bool {
    node.as_keyword_hash_node().is_some() || node.as_hash_node().is_some()
}

/// `attach_function`'s options hash -> whether the call releases the GVL.
///
/// `blocking: true` is the one option with meaning on a target zeo builds for.
/// `convention:` decides between cdecl and stdcall, which is a 32-bit Windows
/// distinction -- the real gem ignores it everywhere else, and so does this.
/// `enums:` and `type_map:` change how VALUES marshal, so an unrecognized or
/// non-literal option stays a clean rejection rather than a silent drop.
fn attach_function_options(node: &Node<'_>) -> PResult<bool> {
    let pairs: Vec<Node<'_>> = match node.as_keyword_hash_node() {
        Some(k) => k.elements().iter().collect(),
        None => node
            .as_hash_node()
            .map(|h| h.elements().iter().collect())
            .unwrap_or_default(),
    };
    let mut blocking = false;
    for pair in pairs {
        let assoc = pair.as_assoc_node().ok_or_else(|| {
            "attach_function's options must be literal `key: value` pairs".to_string()
        })?;
        let key = assoc
            .key()
            .as_symbol_node()
            .map(|s| String::from_utf8_lossy(s.unescaped()).into_owned())
            .ok_or_else(|| {
                "attach_function's options must be literal `key: value` pairs".to_string()
            })?;
        match key.as_str() {
            "blocking" => {
                let v = assoc.value();
                blocking = match () {
                    _ if v.as_true_node().is_some() => true,
                    _ if v.as_false_node().is_some() || v.as_nil_node().is_some() => false,
                    _ => {
                        return Err("attach_function's `blocking:` expects `true` or `false`"
                            .to_string()
                            .into());
                    }
                };
            }
            "convention" => {}
            other => {
                return Err(format!(
                    "attach_function option `{other}:` isn't supported yet (zeo limitation) -- \
                     `blocking:` and `convention:` are"
                )
                .into());
            }
        }
    }
    Ok(blocking)
}

/// A Symbol node's name (`:abs` -> `"abs"`). FFI names/types are always literal
/// symbols; anything else is a clean rejection.
fn ffi_symbol_str(node: &Node<'_>) -> PResult<String> {
    // A string literal names the same thing: the ffi gem calls `.to_sym` on
    // its name arguments, and `attach_function 'rados_seek', ...` is common.
    node.as_symbol_node()
        .map(|s| String::from_utf8_lossy(s.unescaped()).into_owned())
        .or_else(|| {
            node.as_string_node()
                .map(|s| String::from_utf8_lossy(s.unescaped()).into_owned())
        })
        .ok_or_else(|| "expected a literal symbol or string in an FFI declaration".into())
}

/// One FFI type as WRITTEN in a declaration. Three spellings reach a type
/// position:
///
///  - a literal symbol -- `:int`, `:pointer`, or a declared alias;
///  - `Status.by_ref` / `Status.ptr` -- a POINTER to a struct, which is what
///    the C prototype takes and what libffi is handed either way. The struct's
///    own layout never enters the call, so this needs nothing from it. (`.by_value`
///    passes the struct itself and does need the layout, so it stays rejected);
///  - a constant naming a declared `enum`/`typedef`/`callback`, which is how
///    the anonymous `Tag = enum(...)` form is referred to afterwards. Only the
///    LEAF name is looked up: the table is keyed by the name as declared, and
///    these are always written inside the library module that declared them.
fn ffi_type_node(
    node: &Node<'_>,
    aliases: &std::collections::HashMap<String, crate::hir::FfiType>,
) -> PResult<crate::hir::FfiType> {
    if let Some(call) = node.as_call_node()
        && let Some(recv) = call.receiver()
        && call.arguments().is_none()
    {
        match call.name().as_slice() {
            b"by_ref" | b"ptr" => return Ok(crate::hir::FfiType::Pointer),
            b"by_value" | b"val" => {
                // The receiver must be a struct whose `layout` already
                // lowered -- the by-value ABI needs the full field list.
                let path = const_path_string(&recv).unwrap_or_default();
                let leaf = path.rsplit("::").next().unwrap_or(&path);
                return match aliases.get(leaf) {
                    Some(t @ crate::hir::FfiType::Struct(_)) => Ok(t.clone()),
                    _ => Err(format!(
                        "`{path}.by_value` needs an `FFI::Struct` whose `layout` lowered earlier \
                         in this program"
                    )
                    .into()),
                };
            }
            _ => {}
        }
    }
    if let Some(path) = const_path_string(node) {
        let leaf = path.rsplit("::").next().unwrap_or(&path);
        return match aliases.get(leaf) {
            Some(t) => Ok(t.clone()),
            None => Err(format!(
                "`{path}` isn't a declared FFI type (expected an `enum`/`typedef`/`callback` \
                 declared earlier in this library)"
            )
            .into()),
        };
    }
    ffi_type_of(&ffi_symbol_str(node)?, aliases)
}

/// The argument-type list of an `attach_function`, splitting a trailing
/// `:varargs` marker: `[:string, :varargs]` -> `([Str], true)`. `:varargs` is
/// only legal as the final element (a variadic function's fixed prototype ends
/// before it); anywhere else is a clean rejection.
fn ffi_arg_types(
    node: &Node<'_>,
    aliases: &std::collections::HashMap<String, crate::hir::FfiType>,
) -> PResult<(Vec<crate::hir::FfiType>, bool)> {
    let array = node
        .as_array_node()
        .ok_or_else(|| "attach_function's argument list must be a literal array".to_string())?;
    let elems: Vec<Node<'_>> = array.elements().iter().collect();
    let mut types = Vec::new();
    let mut variadic = false;
    for (i, el) in elems.iter().enumerate() {
        // `:varargs` is a marker rather than a type, so it is read off the
        // literal symbol before anything else; every other element is an
        // ordinary type position and may be written any of the three ways.
        if el.as_symbol_node().is_some() && ffi_symbol_str(el)? == "varargs" {
            if i != elems.len() - 1 {
                return Err("`:varargs` must be the last FFI argument type"
                    .to_string()
                    .into());
            }
            variadic = true;
        } else {
            types.push(ffi_type_node(el, aliases)?);
        }
    }
    Ok((types, variadic))
}

/// `[:int, :string]` -> `[Int(32), Str]`. The argument-type list of an
/// `attach_function` (a literal array of type symbols).
fn ffi_type_array(
    node: &Node<'_>,
    aliases: &std::collections::HashMap<String, crate::hir::FfiType>,
) -> PResult<Vec<crate::hir::FfiType>> {
    let array = node
        .as_array_node()
        .ok_or_else(|| "attach_function's argument list must be a literal array".to_string())?;
    array
        .elements()
        .iter()
        .map(|el| ffi_type_node(&el, aliases))
        .collect()
}

/// Map a real `ffi`-gem type keyword to our `FfiType` -- the scalar keyword
/// table lives in `zeo_abi::ffi::CScalar`, shared with the runtime's own
/// varargs/`FFI::Type` resolution so the two sides cannot drift. A keyword
/// misses to the declared aliases, then to the portable C/Win32 typedef
/// table (also `CScalar`'s; the doc comments there say why several POSIX
/// names are deliberately absent). Unknown -> a clean, greppable error.
fn ffi_type_of(
    sym: &str,
    aliases: &std::collections::HashMap<String, crate::hir::FfiType>,
) -> PResult<crate::hir::FfiType> {
    if let Some(s) = zeo_abi::ffi::CScalar::from_keyword(sym) {
        return Ok(s.into());
    }
    if let Some(t) = aliases.get(sym) {
        return Ok(t.clone());
    }
    match zeo_abi::ffi::CScalar::from_c_typedef(sym) {
        Some(s) => Ok(s.into()),
        None => Err(format!(
            "unsupported FFI type `:{sym}` (expected a scalar keyword, `:pointer`, `:string`, a C typedef whose width is the same on every target zeo builds for, or a declared `typedef`/`enum`/`callback` name)"
        ).into()),
    }
}

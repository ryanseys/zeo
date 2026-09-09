//! `lower_ffi_directive` -- the dispatcher for the directives inside an
//! `extend FFI::Library` module -- and its two arms: `ffi_lib` lib-name
//! resolution and `attach_function` lowering.

use super::recognize::{
    body_local_value, const_path_string, ffi_c_name, local_array_elements, local_read_name,
    splat_local_elements, word_list_to_syms,
};
use super::types::{
    TypePos, degrade_callback_struct_refs, ffi_arg_types, ffi_symbol_str, ffi_type_array,
    ffi_type_node, ffi_type_of,
};
use crate::hir::{Hir, HirNode, NodeId, Params, Visibility};
use crate::lower::PResult;
use crate::lower::literals::assemble_i64;
use ruby_prism::{Node, ParseResult};

/// Lower one directive inside an FFI-library module. Returns `true` if it WAS an
/// FFI directive (`ffi_lib` / `attach_function`), `false` to fall through to the
/// ordinary class-body lowering.
pub(crate) fn lower_ffi_directive(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
    ffi_lib: &mut crate::hir::FfiLib,
    aliases: &mut crate::compiler::FMap<String, crate::hir::FfiType>,
    out: &mut Vec<NodeId>,
    class_body: &[Node<'_>],
) -> PResult<bool> {
    // Catch up on types declared since this body's snapshot: a struct or
    // typedef declared by a NESTED class body mid-module (sha3 nests its
    // state struct inside the library module, above the `attach_function`s
    // that pass it).
    for (k, v) in hir.inherited_ffi_types() {
        match aliases.get(&k) {
            // A by-reference placeholder YIELDS to a real layout. A struct
            // class is known before its `layout` lowers -- so a body seeded
            // when only the name existed held `StructRef`, and a plain
            // `or_insert` could never replace it, which made a `.by_value`
            // further down the same body report a layout it now has.
            Some(crate::hir::FfiType::StructRef(_))
                if matches!(v, crate::hir::FfiType::Struct(_)) => {}
            Some(_) => continue,
            None => {}
        }
        aliases.insert(k, v);
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
        let (tag, ty) = enum_declaration(result, hir, &args, out, class_body)?;
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
    // program-wide table (`FfiVocab::ffi_types`) is what makes reachable.
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
            let existing = ffi_type_node(&args[0], aliases, TypePos::Signature)?;
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
                    let slot = hir.ffi.ffi_lib_slots;
                    hir.ffi.ffi_lib_slots += 1;
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
                                (1, crate::lower::lower_node(result, hir, &inner)?)
                            }
                            None => (0, crate::lower::lower_node(result, hir, a)?),
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
            let existing = ffi_type_node(&args[0], aliases, TypePos::Signature)?;
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
            // A NAMELESS `enum [:a, :b]` statement registers no type name, so
            // nothing later can reference it in a type position; its one
            // effect -- symbol/int conversion for arguments typed with THAT
            // enum -- is unreachable without a name. Validate the members and
            // consume the statement.
            if let [only] = args.as_slice()
                && only.as_array_node().is_some()
            {
                parse_enum_members(std::slice::from_ref(only), hir, out, class_body)?;
                return Ok(true);
            }
            let (tag, ty) = enum_declaration(result, hir, &args, out, class_body)?;
            let Some(tag) = tag else {
                return Err("enum expects `:tag, [members]` or `[members]`"
                    .to_string()
                    .into());
            };
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
            let arg_types = ffi_type_array(params, aliases, class_body)?;
            let ret_ty = ffi_type_node(ret, aliases, TypePos::Signature)?;
            // `:strptr` is an attach_function RETURN device; a callback's CIF
            // marshals through plain kinds and has no pair wrap.
            if arg_types
                .iter()
                .chain(std::iter::once(&ret_ty))
                .any(|t| matches!(t, crate::hir::FfiType::StrPtr))
            {
                return Err(
                    "`:strptr` is only usable as an `attach_function` return type"
                        .to_string()
                        .into(),
                );
            }
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
            let mut arg_types = arg_types;
            degrade_callback_struct_refs(&mut arg_types, &ret_ty)?;
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
                class_body,
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
/// One `enum` declaration, in either tier: `(tag, type)`.
///
/// The gem decides the shape by RUNTIME CLASS -- `Library#enum` takes the
/// named form when `args[0]` is a Symbol and `args[1]` is an Array, and reads
/// every argument as a member otherwise. Syntax can only approximate that, and
/// the approximation this makes is the one that matches: exactly two
/// arguments, the first a literal symbol, the second a single unsplatted
/// expression. A splat cannot be it -- `enum(:level, *levels)` passes the tag
/// as `args[0]` and the members as `args[1..]`, so `args[1]` is a member, not
/// an Array, and the gem reads the whole list anonymously (oracle-checked: a
/// signature naming `:level` then raises "unable to resolve type").
///
/// When the members fold, the members ARE the type. When they don't -- a
/// helper call, a value read out of a shared library -- the declaration
/// defers: the ABI is `int` either way, so only the marshaling table waits for
/// the class body to run. See [`crate::hir::FfiType::EnumSlot`].
fn enum_declaration<'a>(
    result: &ParseResult,
    hir: &mut Hir,
    args: &[Node<'a>],
    out: &mut Vec<NodeId>,
    class_body: &[Node<'a>],
) -> PResult<(Option<String>, crate::hir::FfiType)> {
    let named = match args {
        [tag, members] if tag.as_symbol_node().is_some() && members.as_splat_node().is_none() => {
            Some((ffi_symbol_str(tag)?, members))
        }
        _ => None,
    };
    let (tag, member_nodes) = match &named {
        Some((tag, members)) => (Some(tag.clone()), std::slice::from_ref(*members)),
        None => (None, args),
    };
    if let Ok(members) = parse_enum_members(member_nodes, hir, out, class_body) {
        return Ok((tag, crate::hir::FfiType::Enum(members)));
    }
    let slot = hir.ffi.ffi_enum_slots;
    hir.ffi.ffi_enum_slots += 1;
    let slot_lit = hir.push(HirNode::IntegerLit(slot as i64));
    let mut call_args = vec![crate::hir::ArrayElem::Single(slot_lit)];
    for m in member_nodes {
        // A splat needs no marker: `enum_store` flattens every argument, which
        // is also what `easy_options(:enum).to_a.flatten` relies on.
        let id = match m.as_splat_node() {
            Some(s) => {
                let inner = s.expression().ok_or_else(|| {
                    "enum can't forward a bare `*` splat (zeo limitation)".to_string()
                })?;
                crate::lower::lower_node(result, hir, &inner)?
            }
            None => crate::lower::lower_node(result, hir, m)?,
        };
        call_args.push(crate::hir::ArrayElem::Single(id));
    }
    out.push(hir.push(HirNode::Call {
        receiver: None,
        name: "__zeo_ffi_enum".to_string(),
        args: call_args,
        kwargs: Vec::new(),
        block: None,
        block_arg: None,
        safe: false,
    }));
    Ok((tag, crate::hir::FfiType::EnumSlot(slot)))
}

pub(super) fn parse_enum_members<'a>(
    args: &[Node<'a>],
    hir: &Hir,
    body_so_far: &[NodeId],
    class_body: &[Node<'a>],
) -> PResult<Vec<(String, i64)>> {
    // NOT splat-expanded: `enum(:level, *levels)` is the gem's ANONYMOUS
    // form (every argument is a member, `:level` included), not a named enum
    // over the array -- oracle-checked, it raises "unable to resolve type
    // 'level'" at the first signature that names the tag.
    //
    // The members are either one literal array or the argument list itself --
    // `enum :tag, [:a, :b]` and `enum(:a, :b)` both reach here.
    let unwrapped: Vec<Node<'a>>;
    let elems: &[Node<'a>] = match args {
        [one] if word_list_to_syms(one).is_some() => {
            unwrapped = word_list_to_syms(one).expect("just matched");
            &unwrapped
        }
        // `enum :colour, members` -- the list is a body-local the class body
        // built up, the same value `layout(*members)` reads.
        [one]
            if local_read_name(one)
                .and_then(|n| local_array_elements(&n, class_body, one.location().start_offset()))
                .is_some() =>
        {
            unwrapped = local_read_name(one)
                .and_then(|n| local_array_elements(&n, class_body, one.location().start_offset()))
                .expect("just matched");
            &unwrapped
        }
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
        // The next element is a VALUE unless it reads as a member name (a
        // literal symbol/string). Deciding by shape first keeps the error
        // honest: deciding by foldability would re-read an unfoldable value
        // as the next member and reject it as "expected a literal symbol",
        // naming the wrong rule.
        let value = match elems.get(i) {
            Some(n) if n.as_symbol_node().is_none() && n.as_string_node().is_none() => {
                i += 1;
                ffi_const_int(n, hir, body_so_far).ok_or_else(|| {
                    format!(
                        "enum member `{name}`'s value must be an integer literal or a \
                         constant already set to one (zeo limitation)"
                    )
                })?
            }
            _ => next,
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
pub(crate) fn ffi_const_int(node: &Node<'_>, hir: &Hir, body_so_far: &[NodeId]) -> Option<i64> {
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
    // `BreakdownStepStruct.size * MAX_TIERS` -- an EARLIER struct's extent,
    // which zeo laid out itself, so it is as much a compile-time constant as
    // the literal the gem could have written. j-law-ruby sizes its inline
    // storage arrays this way. `.alignment` for the same reason.
    if args.is_empty()
        && matches!(call.name().as_slice(), b"size" | b"alignment")
        && let Some(path) = const_path_string(&recv)
        && let Some(layout) = hir
            .ffi
            .ffi_struct_layouts
            .get(path.rsplit("::").next().unwrap_or(&path))
    {
        let n = match call.name().as_slice() {
            b"size" => layout.size,
            _ => layout.align,
        };
        return i64::try_from(n).ok();
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
/// One declared field: name, type, and the byte offset the declaration PINNED
/// it to, if it named one. See [`as_ffi_layout`].
pub(crate) type FfiField = (String, crate::hir::FfiType, Option<usize>);

pub(crate) fn as_ffi_layout<'a>(
    node: &Node<'a>,
    aliases: &crate::compiler::FMap<String, crate::hir::FfiType>,
    hir: &Hir,
    body_so_far: &[NodeId],
    class_body: &[Node<'a>],
) -> PResult<Option<Vec<FfiField>>> {
    let Some(call) = node.as_call_node() else {
        return Ok(None);
    };
    if call.receiver().is_some() || call.name().as_slice() != b"layout" {
        return Ok(None);
    }
    let args: Vec<Node<'a>> = call
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    // `layout(*members)` -- the field list is a body-local array the class
    // body built up. Reading it back is what lets a conditionally-shaped
    // struct lower at all.
    let args = match splat_local_elements(&args, class_body, node) {
        Some(elems) => elems,
        None => args,
    };
    let field =
        |name_node: &Node<'_>, ty_node: &Node<'_>| -> PResult<(String, crate::hir::FfiType)> {
            let name = ffi_symbol_str(name_node)?;
            let ty = match layout_array_type(ty_node, aliases, hir, body_so_far)? {
                Some(t) => t,
                // A body constant holding a type symbol resolves to what it
                // names; anything else takes the ordinary type-node path.
                None => match body_const_type_symbol(hir, body_so_far, ty_node) {
                    Some(sym) => ffi_type_of(&sym, aliases)?,
                    None => ffi_type_node(ty_node, aliases, TypePos::Field)?,
                },
            };
            // A TYPEDEF'D struct name resolves through `find_type` to the
            // by-reference wrapper, so as a field it is a plain pointer.
            // A bare struct CLASS written here means embed INLINE -- which
            // needs the layout zeo never saw (a DSL-built one), so it stays
            // a loud rejection rather than a silently mis-shaped field.
            let ty = match ty {
                crate::hir::FfiType::StructRef(path) => {
                    if const_path_string(ty_node).is_some() {
                        return Err(format!(
                            "`{path}`'s layout isn't known to zeo (its `layout` directive \
                             never lowered), so it can't be embedded inline as a field"
                        )
                        .into());
                    }
                    crate::hir::FfiType::Pointer
                }
                t => t,
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
            let (name, ty) = field(k, v)?;
            fields.push((name, ty, None));
        }
        return Ok(Some(fields));
    }
    if args.is_empty() {
        return Err("FFI::Struct `layout` expects `:name, :type` pairs"
            .to_string()
            .into());
    }
    let mut fields = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let Some(ty_node) = args.get(i + 1) else {
            return Err("FFI::Struct `layout` expects `:name, :type` pairs"
                .to_string()
                .into());
        };
        let (name, ty) = field(&args[i], ty_node)?;
        i += 2;
        // The gem's THIRD element per field: an explicit byte offset
        // (`layout :Type, :int16, 0, :Size, :int32, 2` -- every Win32 header
        // struct in winwindow). Decided by SHAPE first, like an enum member's
        // value: a name is a symbol or a string, so anything else in that
        // slot is the offset, and a non-foldable one is an honest rejection
        // rather than a mis-read field name.
        let offset =
            match args.get(i) {
                Some(n) if n.as_symbol_node().is_none() && n.as_string_node().is_none() => {
                    i += 1;
                    let off = ffi_const_int(n, hir, body_so_far).ok_or_else(|| {
                        format!(
                            "field `{name}`'s explicit offset must be an integer literal, or a \
                         constant already set to one (zeo limitation)"
                        )
                    })?;
                    Some(usize::try_from(off).map_err(|_| {
                        format!("field `{name}`'s explicit offset can't be negative")
                    })?)
                }
                _ => None,
            };
        fields.push((name, ty, offset));
    }
    Ok(Some(fields))
}

/// A hash literal's `(key, value)` node pairs -- braced or keyword form. Any
/// non-pair element (a `**splat`) declines the whole hash.
pub(super) fn hash_pairs<'a>(node: &Node<'a>) -> Option<Vec<(Node<'a>, Node<'a>)>> {
    let elements: Vec<Node<'a>> = if let Some(h) = node.as_hash_node() {
        h.elements().iter().collect()
    } else {
        let h = node.as_keyword_hash_node()?;
        h.elements().iter().collect()
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
    aliases: &crate::compiler::FMap<String, crate::hir::FfiType>,
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
    // The element type takes the same body-constant fold as a plain field
    // (`[WCHAR_T, CCHARW_MAX]` -- both halves are constants in ffi-ncurses),
    // and may itself be an inline array: X11's `XTransform` is `layout
    // :matrix, [[:XFixed, 3], 3]`, a 3x3 matrix of fixed-point values. C lays
    // a 2-D array out as rows of rows, which is exactly this nesting.
    let elem = match layout_array_type(&elems[0], aliases, hir, body_so_far)? {
        Some(inner) => inner,
        None => match body_const_type_symbol(hir, body_so_far, &elems[0]) {
            Some(sym) => ffi_type_of(&sym, aliases)?,
            None => ffi_type_node(&elems[0], aliases, TypePos::Field)?,
        },
    };
    // Same rule as a plain field's: a typedef'd struct name is the
    // by-reference wrapper (one pointer per element); a bare layout-less
    // struct CLASS would embed inline, which needs the layout.
    let elem = match elem {
        crate::hir::FfiType::StructRef(path) => {
            if const_path_string(&elems[0]).is_some() {
                return Err(format!(
                    "`{path}`'s layout isn't known to zeo (its `layout` directive never \
                     lowered), so it can't be embedded inline as an array element"
                )
                .into());
            }
            crate::hir::FfiType::Pointer
        }
        t => t,
    };
    let count = ffi_const_int(&elems[1], hir, body_so_far).ok_or_else(|| {
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
    let wanted = const_leaf_name(node)?;
    let own = body_so_far.iter().rev().find_map(|&id| match &hir[id] {
        HirNode::ConstWrite { name, value, .. } if *name == wanted => match hir[*value] {
            HirNode::IntegerLit(n) => Some(n),
            _ => None,
        },
        _ => None,
    });
    // An ENCLOSING body's constant (ffi-ncurses spells its counts in the
    // module wrapping the struct) reaches here through the recorded side
    // map -- see `FfiVocab::ffi_int_consts` for the poison rule.
    own.or_else(|| hir.ffi.ffi_int_consts.get(&wanted).copied().flatten())
}

/// The leaf name of a bare (`LEN`) or QUALIFIED (`Limits::LEN`) constant read.
/// A qualified read reduces to its leaf on purpose: the side maps these feed
/// (`ffi_int_consts`/`ffi_symbol_consts`) are leaf-keyed with a
/// poison-on-conflict rule, the same reduction the FFI type table applies.
fn const_leaf_name(node: &Node<'_>) -> Option<String> {
    let path = const_path_string(node)?;
    Some(path.rsplit("::").next().unwrap_or(&path).to_string())
}

/// A body constant holding a type SYMBOL (`NCURSES_ATTR_T = :int` above a
/// `layout :attr, NCURSES_ATTR_T` -- ffi-ncurses spells its whole layout
/// vocabulary this way). The symbol's NAME comes back for the ordinary
/// keyword resolution; `body_const_int`'s sibling, with the same
/// enclosing-body fallback.
fn body_const_type_symbol(hir: &Hir, body_so_far: &[NodeId], node: &Node<'_>) -> Option<String> {
    let wanted = const_leaf_name(node)?;
    let own = body_so_far.iter().rev().find_map(|&id| match &hir[id] {
        HirNode::ConstWrite { name, value, .. } if *name == wanted => match &hir[*value] {
            HirNode::SymbolLit(s) => Some(s.clone()),
            _ => None,
        },
        _ => None,
    });
    own.or_else(|| hir.ffi.ffi_symbol_consts.get(&wanted).cloned().flatten())
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
            } else {
                let embedded = part.as_embedded_statements_node()?;
                let stmts: Vec<Node<'_>> = embedded
                    .statements()
                    .map(|s| s.body().iter().collect())
                    .unwrap_or_default();
                let [only] = stmts.as_slice() else {
                    return None;
                };
                out.push_str(&fold_lib_string(only, hir)?);
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
            other @ (Component::Prefix(_) | Component::RootDir | Component::Normal(_)) => {
                out.push(other)
            }
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
    aliases: &crate::compiler::FMap<String, crate::hir::FfiType>,
    class_body: &[Node<'_>],
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
            let name = ffi_c_name(&positional[0], class_body)?;
            (name.clone(), name, &positional[1], &positional[2])
        }
        4 => (
            ffi_symbol_str(&positional[0])?,
            ffi_c_name(&positional[1], class_body)?,
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
    let (arg_types, variadic) = ffi_arg_types(types_node, aliases, class_body)?;
    let ret = ffi_type_node(ret_node, aliases, TypePos::Signature)?;
    let blocking = match options {
        Some(opts) => attach_function_options(opts, class_body)?,
        None => false,
    };
    // A struct REFERENCE degrades here, at the declaration: an argument is
    // the plain pointer (`to_pointer` already auto-converts a struct via
    // `to_ptr`, the gem's own rule), and a RETURN is a plain `FFI::Pointer`
    // -- oracle-verified, the gem does NOT auto-wrap a returned pointer in
    // the class. Codegen never sees `StructRef`.
    let arg_types: Vec<crate::hir::FfiType> = arg_types
        .into_iter()
        .map(|t| match t {
            crate::hir::FfiType::StructRef(_) => crate::hir::FfiType::Pointer,
            t => t,
        })
        .collect();
    let ret = match ret {
        crate::hir::FfiType::StructRef(_) => crate::hir::FfiType::Pointer,
        ret => ret,
    };
    // By-value structs ride the fixed `extern "C"` tier, where rustc owns the
    // ABI -- as arguments AND as a return (the wrapper below views the
    // returned bytes through the struct's own class). The positions the
    // other tiers cannot express are clean rejections at the declaration --
    // never a wrong call.
    let ret_struct_class = match &ret {
        crate::hir::FfiType::Struct(l) if !l.class_path.is_empty() => Some(l.class_path.clone()),
        crate::hir::FfiType::Struct(_) => {
            return Err(
                "returning a struct BY VALUE needs a NAMED `FFI::Struct` class to wrap it in \
                 (zeo limitation) -- return `.by_ref` (a pointer) and wrap it"
                    .to_string()
                    .into(),
            );
        }
        _ => None,
    };
    // `:strptr` reads a RETURNED `char *` twice (string + pointer); the gem
    // has no argument meaning for it either.
    if arg_types
        .iter()
        .any(|t| matches!(t, crate::hir::FfiType::StrPtr))
    {
        return Err(
            "`:strptr` is only usable as an `attach_function` return type"
                .to_string()
                .into(),
        );
    }
    // A UNION by value has no honest aggregate descriptor on either tier
    // (the mirror's field asserts would overlap; libffi has no union type),
    // and the extern tier's mirror asserts would reject it at build time
    // anyway -- say it here, at the declaration.
    let union_by_value = std::iter::once(&ret)
        .chain(arg_types.iter())
        .any(|t| matches!(t, crate::hir::FfiType::Struct(l) if l.union));
    if union_by_value {
        return Err(
            "a union passed or returned BY VALUE isn't supported yet (zeo limitation) -- \
             use `.by_ref`"
                .to_string()
                .into(),
        );
    }
    let by_value = ret_struct_class.is_some()
        || arg_types
            .iter()
            .any(|t| matches!(t, crate::hir::FfiType::Struct(_)));
    if by_value && variadic {
        return Err(
            "a variadic `attach_function` can't pass or return a struct BY VALUE (zeo \
             limitation) -- use `.by_ref`"
                .to_string()
                .into(),
        );
    }
    // `blocking: true` beside a callback argument is fine in every mode: the
    // default parallel mode has no GVL to release, and under `ZEO_GVL=1` the
    // callback trampoline re-acquires before entering ruby (see
    // `zeo_rt::ffi::invoke_callback`). rdkafka's poll loop is the shape.

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
    let ffi_node = hir.push(HirNode::Ffi(Box::new(crate::hir::FfiCall {
        symbol: c_symbol,
        lib,
        args: call_args,
        ret,
        variadic: variadic_read,
        blocking,
    })));
    // A by-value struct return comes out of the call as a fresh ruby-owned
    // `MemoryPointer` (see codegen's `emit_ffi_call`); the struct class's own
    // `new(pointer)` then views it -- the same wrap the accessor synthesis
    // uses for a nested field, resolved lexically from the attach site.
    let body = match ret_struct_class {
        Some(class_name) => vec![hir.push(HirNode::New {
            class_name,
            args: vec![ffi_node],
            kwargs: Vec::new(),
            block: None,
        })],
        None => vec![ffi_node],
    };
    let params = Params {
        required: param_names,
        rest: variadic.then(|| Some("__ffi_rest".to_string())),
        ..Default::default()
    };
    Ok(hir.push(HirNode::DefMethod {
        name: ruby_name,
        params: Box::new(params),
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
fn attach_function_options<'a>(node: &Node<'a>, class_body: &[Node<'a>]) -> PResult<bool> {
    let elements: Vec<Node<'a>> = match node.as_keyword_hash_node() {
        Some(k) => k.elements().iter().collect(),
        None => node
            .as_hash_node()
            .map(|h| h.elements().iter().collect())
            .unwrap_or_default(),
    };
    // `**opts`, where the class body set `opts = { blocking: true }` above --
    // cztop, mosq, jansson and czmq-ffi-gen all hoist the one option they
    // share into a local and splat it into every declaration. Resolved
    // through the same body-local replay a computed C name uses.
    let mut pairs: Vec<Node<'a>> = Vec::new();
    for element in elements {
        let Some(splat) = element.as_assoc_splat_node() else {
            pairs.push(element);
            continue;
        };
        let hash = splat
            .value()
            .as_ref()
            .and_then(local_read_name)
            .and_then(|n| body_local_value(&n, class_body, node.location().end_offset()))
            .and_then(|v| v.as_hash_node().map(|h| h.elements().iter().collect()))
            .ok_or_else(|| {
                "attach_function's options must be literal `key: value` pairs".to_string()
            })?;
        pairs.extend::<Vec<Node<'a>>>(hash);
    }
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

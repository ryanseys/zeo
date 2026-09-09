//! Keyword -> `FfiType` resolution: the C type-name mapping every FFI
//! declaration and signature position shares.

use super::recognize::{
    const_path_string, const_read_name, const_value_node, local_array_elements, local_read_name,
    platform_scalar_of, unwrap_freeze, unwrap_parens, word_list_to_syms,
};
use crate::lower::PResult;
use crate::lower::literals::assemble_i64;
use ruby_prism::Node;

/// The node an FFI declaration position really reads, past the spellings that
/// only WRAP a value. ffi-tk writes `attach_function :Tk_GetColor, [:pointer,
/// :pointer, name = :string], :pointer` -- an assignment mid-list, whose value
/// is the type and whose local is used further down the file. Parentheses wrap
/// the same way (`typedef (COND ? :long_long : :long), :json_int`).
fn ffi_written_value<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    let unwrap_once = |n: &Node<'a>| -> Option<Node<'a>> {
        if let Some(w) = n.as_local_variable_write_node() {
            return Some(w.value());
        }
        let body: Vec<Node<'a>> = n
            .as_parentheses_node()?
            .body()?
            .as_statements_node()?
            .body()
            .iter()
            .collect();
        <[Node<'a>; 1]>::try_from(body).ok().map(|[only]| only)
    };
    let mut cur = unwrap_once(node)?;
    while let Some(inner) = unwrap_once(&cur) {
        cur = inner;
    }
    Some(cur)
}

/// A Symbol node's name (`:abs` -> `"abs"`). FFI names/types are always literal
/// symbols; anything else is a clean rejection.
pub(super) fn ffi_symbol_str(node: &Node<'_>) -> PResult<String> {
    let written = ffi_written_value(node);
    let node = written.as_ref().unwrap_or(node);
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
///
/// A callback ARGUMENT naming a struct class degrades to a pointer, which is
/// what the Proc really receives.
///
/// ruby-ffi does NOT hand the Proc a Struct instance there.
/// `StructByReference#from_native` would build one, but the callback path
/// never routes an argument through the Ruby data converter --
/// oracle-verified with `qsort` over a `[Pair, Pair]` comparator, whose block
/// receives an `FFI::Pointer`. Wrapping the pointer is the gem's own job.
///
/// A RETURN in that position stays rejected: nothing here measured it, and a
/// silently wrong conversion is worse than a refusal.
pub(super) fn degrade_callback_struct_refs(
    arg_types: &mut [crate::hir::FfiType],
    ret_ty: &crate::hir::FfiType,
) -> PResult<()> {
    if matches!(ret_ty, crate::hir::FfiType::StructRef(_)) {
        return Err(
            "a callback RETURNING a struct class isn't supported yet (zeo limitation) \
             -- return `:pointer` and wrap it in the struct class yourself"
                .to_string()
                .into(),
        );
    }
    for t in arg_types {
        if matches!(t, crate::hir::FfiType::StructRef(_)) {
            *t = crate::hir::FfiType::Pointer;
        }
    }
    Ok(())
}

/// Where a type expression is WRITTEN, which decides what a bare struct name
/// means: ruby-ffi's `find_type` wraps a struct class as `StructByReference`
/// (a pointer) in a signature/typedef position, while a layout FIELD naming a
/// struct class embeds it inline, by value.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TypePos {
    Signature,
    Field,
}

pub(super) fn ffi_type_node(
    node: &Node<'_>,
    aliases: &crate::compiler::FMap<String, crate::hir::FfiType>,
    pos: TypePos,
) -> PResult<crate::hir::FfiType> {
    let written = ffi_written_value(node);
    let node = written.as_ref().unwrap_or(node);
    // An INLINE `callback([...], ret)` in a type position -- the anonymous
    // twin of the named `callback :tag, [...], ret` declaration, carrying the
    // same signature without registering a name.
    if let Some(call) = node.as_call_node()
        && call.receiver().is_none()
        && call.name().as_slice() == b"callback"
        && let Some(arguments) = call.arguments()
    {
        let args: Vec<Node<'_>> = arguments.arguments().iter().collect();
        let [params, ret] = args.as_slice() else {
            return Err(
                "callback expects `[arg_types], return_type` in a type position"
                    .to_string()
                    .into(),
            );
        };
        // No class body to replay here: `ffi_type_node` is reached from every
        // type position, and threading one through all of them for the
        // anonymous-callback arm alone is not worth it. The literal, `%i[..]`
        // and `[..] * n` spellings still resolve; only a body-local list
        // written inside an INLINE callback would not.
        let mut arg_types = ffi_type_array(params, aliases, &[])?;
        let ret_ty = ffi_type_node(ret, aliases, TypePos::Signature)?;
        degrade_callback_struct_refs(&mut arg_types, &ret_ty)?;
        return Ok(crate::hir::FfiType::Callback(arg_types, Box::new(ret_ty)));
    }
    if let Some(call) = node.as_call_node()
        && call.receiver().is_some()
    {
        // The ffi gem's DIRECTION annotations -- `Struct.ptr(:in)`,
        // `Status.in`/`.out`/`.inout`, `Foo.ptr.out` -- all pass a POINTER;
        // the direction only tunes the gem's own marshaling copies, never
        // the ABI, so every spelling is the plain pointer type here.
        let direction_arg = call.arguments().is_none_or(|a| {
            let args: Vec<Node<'_>> = a.arguments().iter().collect();
            matches!(args.as_slice(), [d] if d.as_symbol_node().is_some())
        });
        match call.name().as_slice() {
            b"by_ref" | b"ptr" if direction_arg => return Ok(crate::hir::FfiType::Pointer),
            b"in" | b"out" | b"inout" if call.arguments().is_none() => {
                return Ok(crate::hir::FfiType::Pointer);
            }
            _ => {}
        }
    }
    if let Some(call) = node.as_call_node()
        && let Some(recv) = call.receiver()
        && call.arguments().is_none()
    {
        match call.name().as_slice() {
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
        // `FFI::Type::UINT32` written where a type goes. ruby-ffi's
        // `find_type` takes the OBJECT, so the path names itself and no
        // library has to have declared it.
        if let Some(t) = super::ffi_type_constant_of(&path) {
            return Ok(t);
        }
        let leaf = path.rsplit("::").next().unwrap_or(&path);
        return match aliases.get(leaf) {
            // A bare struct name in a signature is ruby-ffi's
            // `StructByReference` -- a POINTER, never the by-value ABI
            // (`.by_value` spells that). A LAYOUT field keeps the inline
            // by-value struct.
            Some(crate::hir::FfiType::Struct(l)) if pos == TypePos::Signature => {
                Ok(crate::hir::FfiType::StructRef(l.class_path.clone()))
            }
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
pub(super) fn ffi_arg_types<'a>(
    node: &Node<'a>,
    aliases: &crate::compiler::FMap<String, crate::hir::FfiType>,
    class_body: &[Node<'a>],
) -> PResult<(Vec<crate::hir::FfiType>, bool)> {
    let (elems, repeat) = type_list(node, class_body)
        .ok_or_else(|| "attach_function's argument list must be a literal array".to_string())?;
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
            types.push(ffi_type_node(el, aliases, TypePos::Signature)?);
        }
    }
    Ok((repeated(types, repeat), variadic))
}

/// `list` laid end to end `n` times. `[T]::repeat` wants `Copy`, which an
/// `FfiType` carrying a whole struct layout is not.
fn repeated(list: Vec<crate::hir::FfiType>, n: usize) -> Vec<crate::hir::FfiType> {
    match n {
        1 => list,
        _ => (0..n).flat_map(|_| list.iter().cloned()).collect(),
    }
}

/// The ELEMENTS of a declared type list, and how many times the list repeats.
///
/// Three spellings beyond the plain literal array, all of them a gem writing
/// out a prototype it did not want to spell twice: `[:pointer] * 13` (rbmetis'
/// METIS bindings, csspool's croco callbacks), a body-local the class set to
/// one (`params = %i[string int int]`, ires), and a `%i[...]` word list. The
/// repeat rides back as a COUNT rather than repeated nodes: a prism `Node` is
/// not `Clone`, and repeating the resolved types is the same answer.
fn type_list<'a>(node: &Node<'a>, class_body: &[Node<'a>]) -> Option<(Vec<Node<'a>>, usize)> {
    // `[:float].freeze` -- the spelling a shared prototype gets when it is
    // written as a constant. The freeze is invisible to a type list, which
    // only ever reads the elements.
    // `([:float] * 2).freeze` -- the two wrappers a shared prototype picks up
    // on its way to a constant, neither of which a type list can see.
    if let Some(inner) = unwrap_freeze(node).or_else(|| unwrap_parens(node)) {
        return type_list(&inner, class_body);
    }
    if let Some(syms) = word_list_to_syms(node) {
        return Some((syms, 1));
    }
    if let Some(array) = node.as_array_node() {
        return Some((array.elements().iter().collect(), 1));
    }
    if let Some(name) = local_read_name(node)
        && let Some(elems) = local_array_elements(&name, class_body, node.location().start_offset())
    {
        return Some((elems, 1));
    }
    if let Some(name) = const_read_name(node)
        && let Some(value) = const_value_node(&name, class_body, node.location().start_offset())
    {
        // Read with the SAME rules: `TYPES = ([:float] * 2).freeze` is every
        // form this function already knows, one indirection away. A constant
        // bound to itself terminates on the offset test -- the write is never
        // before its own read.
        return type_list(&value, class_body);
    }
    // `[...] * n`
    let call = node.as_call_node()?;
    if call.name().as_slice() != b"*" {
        return None;
    }
    let args: Vec<Node<'a>> = call.arguments()?.arguments().iter().collect();
    let [count] = args.as_slice() else {
        return None;
    };
    let value = count.as_integer_node()?.value();
    let (negative, digits) = value.to_u32_digits();
    let count = usize::try_from(assemble_i64(negative, digits)?).ok()?;
    let (elems, _) = type_list(&call.receiver()?, class_body)?;
    Some((elems, count))
}

/// `[:int, :string]` -> `[Int(32), Str]`. A declared type list in a position
/// with no `:varargs` marker (a `callback` signature).
pub(super) fn ffi_type_array<'a>(
    node: &Node<'a>,
    aliases: &crate::compiler::FMap<String, crate::hir::FfiType>,
    class_body: &[Node<'a>],
) -> PResult<Vec<crate::hir::FfiType>> {
    let (elems, repeat) = type_list(node, class_body)
        .ok_or_else(|| "attach_function's argument list must be a literal array".to_string())?;
    let types: Vec<crate::hir::FfiType> = elems
        .iter()
        .map(|el| ffi_type_node(el, aliases, TypePos::Signature))
        .collect::<PResult<_>>()?;
    Ok(repeated(types, repeat))
}

/// Map a real `ffi`-gem type keyword to our `FfiType` -- the scalar keyword
/// table lives in `zeo_abi::ffi::CScalar`, shared with the runtime's own
/// varargs/`FFI::Type` resolution so the two sides cannot drift. A keyword
/// misses to the declared aliases, then to the portable C/Win32 typedef
/// table (also `CScalar`'s; the doc comments there say why several POSIX
/// names are deliberately absent). Unknown -> a clean, greppable error.
pub(crate) fn ffi_type_of(
    sym: &str,
    aliases: &crate::compiler::FMap<String, crate::hir::FfiType>,
) -> PResult<crate::hir::FfiType> {
    if let Some(s) = zeo_abi::ffi::CScalar::from_keyword(sym) {
        return Ok(s.into());
    }
    if sym == "strptr" {
        return Ok(crate::hir::FfiType::StrPtr);
    }
    if let Some(t) = aliases.get(sym) {
        return Ok(t.clone());
    }
    if let Some(s) = zeo_abi::ffi::CScalar::from_c_typedef(sym) {
        return Ok(s.into());
    }
    // The POSIX integer typedefs whose width GENUINELY differs between the
    // targets zeo builds for -- `CScalar::from_c_typedef`'s documented
    // rejections. Legal in argument/return position, where the generated
    // code spells the target's own `libc::<name>`; a struct layout still
    // rejects them (see `ffi_field_accessor`).
    let bare = sym.strip_prefix("__").unwrap_or(sym);
    if platform_scalar_of(bare).is_some() {
        return Ok(crate::hir::FfiType::PlatformScalar(bare.to_string()));
    }
    Err(format!(
        "unsupported FFI type `:{sym}` (expected a scalar keyword, `:pointer`, `:string`, a C typedef whose width is the same on every target zeo builds for, or a declared `typedef`/`enum`/`callback` name)"
    ).into())
}

//! The FFI directive family (the real `ffi` gem's idioms): `extend
//! FFI::Library` recognition, `ffi_lib`/`typedef`/`enum`/`callback`/
//! `attach_function` lowering, the `class < FFI::Struct` `layout` ->
//! accessor-method synthesis, and the C type-name mapping shared by both.
//! Split out of `parse/mod.rs`.

use super::{PResult, assemble_i64};
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
    ffi_lib: &mut Option<String>,
    aliases: &mut std::collections::HashMap<String, crate::hir::FfiType>,
    out: &mut Vec<NodeId>,
) -> PResult<bool> {
    // `SassTag = enum(:sass_boolean, :sass_number, ...)` -- the ANONYMOUS enum,
    // named by the constant it is assigned to rather than by a `:tag` argument.
    // sassc and google-protobuf both declare every one of their enums this way,
    // and then use the constant as a field/argument type.
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
        let ty = crate::hir::FfiType::Enum(parse_enum_members(&args)?);
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
    if call.receiver().is_some() {
        return Ok(false);
    }
    let args: Vec<Node<'_>> = call
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    match call.name().as_slice() {
        b"ffi_lib" => {
            // `ffi_lib "m"` / `ffi_lib FFI::Library::LIBC`. The most-recently
            // declared library links every subsequent `attach_function`.
            if let Some(first) = args.first() {
                *ffi_lib = Some(ffi_lib_name(first)?);
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
                (Some(n), Some(l)) if n.as_symbol_node().is_some() => {
                    (ffi_symbol_str(n)?, parse_enum_members(std::slice::from_ref(l))?)
                }
                _ => {
                    return Err(
                        "enum expects `:tag, [members]` (a nameless `enum [...]` is a follow-on)"
                            .to_string()
                            .into(),
                    );
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
/// the `ffi` gem's `enum` does.
fn parse_enum_members(args: &[Node<'_>]) -> PResult<Vec<(String, i64)>> {
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
        let value = match elems.get(i).and_then(enum_int_literal) {
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

/// An explicit integer enum member value (`0`, `100`), or `None` if the node is
/// not an integer literal (i.e. the next member symbol, or the list's end).
fn enum_int_literal(node: &Node<'_>) -> Option<i64> {
    let int = node.as_integer_node()?;
    let value = int.value();
    let (negative, digits) = value.to_u32_digits();
    assemble_i64(negative, digits)
}

/// Recognize an FFI `layout :name, :type, :name, :type, ...` directive inside a
/// `class < FFI::Struct` body and return its `(field, type)` pairs, or `None`
/// if `node` isn't a `layout` call.
pub(crate) fn as_ffi_layout(
    node: &Node<'_>,
    aliases: &std::collections::HashMap<String, crate::hir::FfiType>,
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
    if args.is_empty() || !args.len().is_multiple_of(2) {
        return Err("FFI::Struct `layout` expects `:name, :type` pairs"
            .to_string()
            .into());
    }
    let mut fields = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let name = ffi_symbol_str(&args[i])?;
        let ty = ffi_type_node(&args[i + 1], aliases)?;
        fields.push((name, ty));
        i += 2;
    }
    Ok(Some(fields))
}

/// The `FFI::MemoryPointer` accessor pair and C layout `(size, align)` for a
/// struct field type. Structs hold scalar/pointer fields; a `:string`/`:bool`/
/// nested-struct field is a clean, greppable rejection (follow-on).
fn ffi_field_accessor(ty: &crate::hir::FfiType) -> PResult<(String, String, usize, usize)> {
    use crate::hir::FfiType::*;
    Ok(match ty {
        Int(w) => (
            format!("get_int{w}"),
            format!("put_int{w}"),
            (*w / 8) as usize,
            (*w / 8) as usize,
        ),
        Uint(w) => (
            format!("get_uint{w}"),
            format!("put_uint{w}"),
            (*w / 8) as usize,
            (*w / 8) as usize,
        ),
        Float(32) => ("get_float32".into(), "put_float32".into(), 4, 4),
        Float(64) => ("get_float64".into(), "put_float64".into(), 8, 8),
        Pointer => ("get_pointer".into(), "put_pointer".into(), 8, 8),
        // An enum field is a C `int` in memory, a bool a one-byte `_Bool`.
        // Neither reads back as the number it stores; the generated accessor
        // converts -- see `synthesize_ffi_struct`.
        Enum(_) => ("get_int32".into(), "put_int32".into(), 4, 4),
        Bool => ("get_int8".into(), "put_int8".into(), 1, 1),
        // A `:string` field is a `char *`: read through the pointer, and NOT
        // writable -- CRuby's ffi raises `Cannot set :string fields`, because
        // storing one would need somewhere to keep the bytes alive.
        Str => ("get_pointer".into(), "put_pointer".into(), 8, 8),
        other => return Err(format!(
            "FFI::Struct field type `{other:?}` isn't supported yet (scalar/pointer fields only)"
        )
        .into()),
    })
}

/// Synthesize the Ruby methods for a `class < FFI::Struct` from its `layout`:
/// `[]`/`[]=` read/write each field at its computed C offset over an owned
/// `FFI::MemoryPointer` ivar, plus `pointer`/`to_ptr`, `size`, `offset_of`, and
/// `members`. Offsets follow C alignment (each field aligned to its own size;
/// total rounded to the max field alignment), matching `ffi 1.17.4` and the C
/// ABI. Returned as source for `parse_and_lower_into`.
pub(crate) fn synthesize_ffi_struct(
    fields: &[(String, crate::hir::FfiType)],
    union: bool,
) -> PResult<String> {
    let round_up = |n: usize, a: usize| -> usize { n.div_ceil(a) * a };
    let mut offset = 0usize;
    let mut max_align = 1usize;
    // A union's members all start at offset 0 and it is as wide as its widest
    // member -- the only two places its layout differs from a struct's.
    let mut widest = 0usize;
    // How a field's stored bytes become a ruby value and back. Most fields are
    // the number itself.
    enum Conv {
        Plain,
        Enum(Vec<(String, i64)>),
        Bool,
        Str,
    }
    // (field, getter, putter, offset, conversion)
    let mut placed: Vec<(String, String, String, usize, Conv)> = Vec::new();
    for (name, ty) in fields {
        let (getter, putter, size, align) = ffi_field_accessor(ty)?;
        let off = if union { 0 } else { round_up(offset, align) };
        let conv = match ty {
            crate::hir::FfiType::Enum(m) => Conv::Enum(m.clone()),
            crate::hir::FfiType::Bool => Conv::Bool,
            crate::hir::FfiType::Str => Conv::Str,
            _ => Conv::Plain,
        };
        placed.push((name.clone(), getter, putter, off, conv));
        offset = off + size;
        widest = widest.max(size);
        max_align = max_align.max(align);
    }
    let total = round_up(if union { widest } else { offset }, max_align);

    // An enum field reads back as its member SYMBOL and accepts either a symbol
    // or the raw integer, which is `Enum#from_native`/`#to_native`. A value with
    // no member keeps its number, exactly as the gem's do.
    let read_arms: String = placed
        .iter()
        .map(|(name, getter, _, off, conv)| {
            let read = format!("@__ffi_ptr.{getter}({off})");
            let read = match conv {
                Conv::Plain => read,
                Conv::Bool => format!("{read} != 0"),
                Conv::Str => format!("((__p = {read}).null? ? nil : __p.read_string)"),
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
            let value = match conv {
                Conv::Plain => "__ffi_value".to_string(),
                Conv::Bool => "(__ffi_value ? 1 : 0)".to_string(),
                Conv::Str => unreachable!("returned above"),
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

    Ok(format!(
        r#"
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

/// The library name for `#[link(name = ..)]` from a `ffi_lib` argument. A string
/// literal is taken verbatim; `FFI::Library::LIBC` maps to the platform C
/// library (`c`, which resolves to libSystem on macOS). A `.so`/`.dylib` suffix
/// and a `lib` prefix are stripped -- rustc wants the bare link name.
fn ffi_lib_name(node: &Node<'_>) -> PResult<String> {
    if let Some(s) = node.as_string_node() {
        let raw = String::from_utf8_lossy(s.unescaped()).into_owned();
        return Ok(strip_lib_name(&raw));
    }
    match const_path_string(node).as_deref() {
        Some("FFI::Library::LIBC") => Ok("c".to_string()),
        _ => Err(
            "ffi_lib expects a string library name or FFI::Library::LIBC"
                .to_string()
                .into(),
        ),
    }
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
    lib: Option<String>,
    aliases: &std::collections::HashMap<String, crate::hir::FfiType>,
) -> PResult<NodeId> {
    let _ = result;
    let (ruby_name, c_symbol, types_node, ret_node) = match args.len() {
        3 => {
            let name = ffi_symbol_str(&args[0])?;
            (name.clone(), name, &args[1], &args[2])
        }
        4 => (
            ffi_symbol_str(&args[0])?,
            ffi_symbol_str(&args[1])?,
            &args[2],
            &args[3],
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

/// A Symbol node's name (`:abs` -> `"abs"`). FFI names/types are always literal
/// symbols; anything else is a clean rejection.
fn ffi_symbol_str(node: &Node<'_>) -> PResult<String> {
    node.as_symbol_node()
        .map(|s| String::from_utf8_lossy(s.unescaped()).into_owned())
        .ok_or_else(|| "expected a literal symbol in an FFI declaration".into())
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
        && call.receiver().is_some()
        && call.arguments().is_none()
    {
        match call.name().as_slice() {
            b"by_ref" | b"ptr" => return Ok(crate::hir::FfiType::Pointer),
            b"by_value" | b"val" => {
                return Err("an FFI struct passed BY VALUE isn't supported yet (zeo limitation) \
                            -- `.by_ref` (a pointer) is"
                    .to_string()
                    .into());
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

/// Map a real `ffi`-gem type keyword to our `FfiType`. Covers the scalar
/// surface plus the gem's spellings (`:string`, `:pointer`, the fixed-width
/// `:intN`/`:uintN`, `:size_t`). LP64 (`:long`/`:ulong` = 64), matching macOS
/// and Linux. Unknown -> a clean, greppable error naming the type.
fn ffi_type_of(
    sym: &str,
    aliases: &std::collections::HashMap<String, crate::hir::FfiType>,
) -> PResult<crate::hir::FfiType> {
    use crate::hir::FfiType::*;
    Ok(match sym {
        "void" => Void,
        "char" | "int8" => Int(8),
        "short" | "int16" => Int(16),
        "int" | "int32" => Int(32),
        "long" | "long_long" | "int64" | "ssize_t" => Int(64),
        "uchar" | "uint8" => Uint(8),
        "ushort" | "uint16" => Uint(16),
        "uint" | "uint32" => Uint(32),
        "ulong" | "ulong_long" | "uint64" | "size_t" => Uint(64),
        "float" => Float(32),
        "double" => Float(64),
        "bool" => Bool,
        "string" => Str,
        "pointer" | "buffer_in" | "buffer_out" | "buffer_inout" => Pointer,
        other => match aliases.get(other) {
            Some(t) => t.clone(),
            None => {
                return Err(format!(
                    "unsupported FFI type `:{other}` (expected a scalar keyword, `:pointer`, `:string`, or a declared `typedef`/`enum`/`callback` name)"
                ).into())
            }
        },
    })
}

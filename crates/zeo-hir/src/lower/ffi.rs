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
            let existing = ffi_type_of(&ffi_symbol_str(&args[0])?, aliases)?;
            let new_name = ffi_symbol_str(&args[1])?;
            aliases.insert(new_name, existing);
            Ok(true)
        }
        b"enum" => {
            // `enum :tag, [:sym, val, :sym, ...]` -- register `:tag` as an enum
            // type usable in a later type list. (An anonymous `enum [...]`,
            // whose bare symbols become module values, is a follow-on.)
            let (tag, list) = match (args.first(), args.get(1)) {
                (Some(n), Some(l)) if n.as_symbol_node().is_some() => (ffi_symbol_str(n)?, l),
                _ => {
                    return Err(
                        "enum expects `:tag, [members]` (anonymous enums are a follow-on)"
                            .to_string()
                            .into(),
                    );
                }
            };
            let members = parse_enum_members(list)?;
            aliases.insert(tag, crate::hir::FfiType::Enum(members));
            Ok(true)
        }
        b"callback" => {
            // `callback :tag, [arg_types], ret_type` -- register `:tag` as a
            // usable type name. A C callback IS a function pointer, so the tag
            // resolves to `:pointer`; the signature is validated (so a typo in
            // it is still an error) but not otherwise carried, since marshalling
            // a Ruby Proc into a C function pointer is a follow-on.
            let (tag, params) = match (args.first(), args.get(1), args.get(2)) {
                (Some(t), Some(p), Some(_)) if t.as_symbol_node().is_some() => {
                    (ffi_symbol_str(t)?, p)
                }
                _ => {
                    return Err("callback expects `:tag, [arg_types], return_type`"
                        .to_string()
                        .into());
                }
            };
            ffi_type_array(params, aliases)?;
            aliases.insert(tag, crate::hir::FfiType::Pointer);
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
fn parse_enum_members(node: &Node<'_>) -> PResult<Vec<(String, i64)>> {
    let array = node
        .as_array_node()
        .ok_or_else(|| "enum members must be a literal array".to_string())?;
    let elems: Vec<Node<'_>> = array.elements().iter().collect();
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
    // Struct field types are the base scalars/pointer -- no per-library aliases.
    let no_aliases = std::collections::HashMap::new();
    let mut fields = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let name = ffi_symbol_str(&args[i])?;
        let ty = ffi_type_of(&ffi_symbol_str(&args[i + 1])?, &no_aliases)?;
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
pub(crate) fn synthesize_ffi_struct(fields: &[(String, crate::hir::FfiType)]) -> PResult<String> {
    let round_up = |n: usize, a: usize| -> usize { n.div_ceil(a) * a };
    let mut offset = 0usize;
    let mut max_align = 1usize;
    // (field, getter, putter, offset)
    let mut placed: Vec<(String, String, String, usize)> = Vec::new();
    for (name, ty) in fields {
        let (getter, putter, size, align) = ffi_field_accessor(ty)?;
        let off = round_up(offset, align);
        placed.push((name.clone(), getter, putter, off));
        offset = off + size;
        max_align = max_align.max(align);
    }
    let total = round_up(offset, max_align);

    let read_arms: String = placed
        .iter()
        .map(|(name, getter, _, off)| {
            format!("        when :{name} then @__ffi_ptr.{getter}({off})\n")
        })
        .collect();
    let write_arms: String = placed
        .iter()
        .map(|(name, _, putter, off)| {
            format!("        when :{name} then @__ffi_ptr.{putter}({off}, __ffi_value)\n")
        })
        .collect();
    let offset_arms: String = placed
        .iter()
        .map(|(name, _, _, off)| format!("        when :{name} then {off}\n"))
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
    let arg_types = ffi_type_array(types_node, aliases)?;
    let ret = ffi_type_of(&ffi_symbol_str(ret_node)?, aliases)?;

    // The wrapper's params: one required positional per C argument, named so a
    // `LocalRead` in the `Ffi` body reaches it.
    let param_names: Vec<String> = (0..arg_types.len())
        .map(|i| format!("__ffi_a{i}"))
        .collect();
    let call_args: Vec<(NodeId, crate::hir::FfiType)> = param_names
        .iter()
        .zip(arg_types)
        .map(|(name, ty)| (hir.push(HirNode::LocalRead(name.clone())), ty))
        .collect();
    let body = vec![hir.push(HirNode::Ffi(crate::hir::FfiCall {
        symbol: c_symbol,
        lib,
        args: call_args,
        ret,
    }))];
    let params = Params {
        required: param_names,
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
        .map(|el| ffi_type_of(&ffi_symbol_str(&el)?, aliases))
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
                    "unsupported FFI type `:{other}` (#204 scalar subset; pointer/struct/callback types are follow-ons)"
                ).into())
            }
        },
    })
}

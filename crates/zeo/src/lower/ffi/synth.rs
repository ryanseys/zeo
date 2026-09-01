//! `class < FFI::Struct` `layout` -> accessor-method synthesis: per-field
//! C layout (size/align/offset), the `FFI::StructLayout` descriptors, and
//! the Ruby accessor source the struct class compiles.

use super::directive::FfiField;
use super::recognize::platform_scalar_of;
use crate::lower::PResult;

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
        if matches!(**elem, Callback(..)) {
            return Err(
                "an inline array of callbacks isn't supported yet (zeo limitation)"
                    .to_string()
                    .into(),
            );
        }
        // A struct element has no scalar accessor pair either -- the proxy
        // constructs the element class VIEWING each slot in place (see the
        // `InlineArray` synthesis) -- but its size and alignment place the
        // following fields the same way a scalar's do.
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
    // A platform typedef in a FIELD resolves to the width it has on the one
    // target zeo emits for -- see `platform_scalar_of`. A signature can leave
    // it to rustc; an offset cannot wait.
    if let crate::hir::FfiType::PlatformScalar(name) = ty
        && let Some(s) = platform_scalar_of(name)
    {
        return ffi_field_accessor(&crate::hir::FfiType::from(s));
    }
    let (get, put) = match ty {
        Int(w) => (format!("get_int{w}"), format!("put_int{w}")),
        Uint(w) => (format!("get_uint{w}"), format!("put_uint{w}")),
        Long => ("get_long".into(), "put_long".into()),
        Ulong => ("get_ulong".into(), "put_ulong".into()),
        Float(w) => (format!("get_float{w}"), format!("put_float{w}")),
        // An enum field is a C `int` in memory, a bool a one-byte `_Bool`.
        // Neither reads back as the number it stores; the generated accessor
        // converts -- see `synthesize_ffi_struct`.
        Enum(_) | EnumSlot(_) => ("get_int32".into(), "put_int32".into()),
        Bool => ("get_int8".into(), "put_int8".into()),
        // A `:string` field is a `char *`: read through the pointer, and NOT
        // writable -- CRuby's ffi raises `Cannot set :string fields`, because
        // storing one would need somewhere to keep the bytes alive. A
        // callback field is a C function pointer in memory; the generated
        // accessor wraps/unwraps `FFI::Function` -- see `Conv::Callback`.
        Str | Pointer | Callback(..) => ("get_pointer".into(), "put_pointer".into()),
        other => {
            return Err(format!("FFI::Struct field type `{other:?}` isn't supported yet").into());
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
/// A field's `FFI::StructLayout::Field` DESCRIPTOR -- the Field subclass and
/// the type object CRuby answers from `.layout.fields`, built from the same
/// walked layout the accessors are built from.
fn ffi_field_descriptor(name: &str, ty: &crate::hir::FfiType, off: usize) -> PResult<String> {
    use crate::hir::FfiType::*;
    let (size, align) = {
        let (_, _, s, a) = ffi_field_accessor(ty)?;
        (s, a)
    };
    let builtin = |n: &str| format!("::FFI::Type::Builtin::{n}");
    let scalar_name = |ty: &crate::hir::FfiType| match ty {
        Int(w) => Some(format!("INT{w}")),
        Uint(w) => Some(format!("UINT{w}")),
        Long => Some("LONG".to_string()),
        Ulong => Some("ULONG".to_string()),
        Float(w) => Some(format!("FLOAT{w}")),
        Bool => Some("BOOL".to_string()),
        Str => Some("STRING".to_string()),
        Pointer => Some("POINTER".to_string()),
        PlatformScalar(n) => platform_scalar_of(n)
            .map(crate::hir::FfiType::from)
            .and_then(|t| match t {
                Int(w) => Some(format!("INT{w}")),
                Uint(w) => Some(format!("UINT{w}")),
                _ => None,
            }),
        _ => None,
    };
    let (class, type_src) = match ty {
        Array(elem, count) => {
            let elem_src = match scalar_name(elem) {
                Some(n) => builtin(&n),
                // An array of structs or of arrays: the element's own
                // descriptor is what `elem_type` should answer with.
                None => match &**elem {
                    Struct(l) => format!("::FFI::StructByValue.new({})", l.class_path),
                    _ => builtin("POINTER"),
                },
            };
            (
                "Array",
                format!("::FFI::ArrayType.new({elem_src}, {count})"),
            )
        }
        Struct(l) => (
            "InnerStruct",
            format!("::FFI::StructByValue.new({})", l.class_path),
        ),
        Callback(args, ret) => {
            let args: Vec<String> = args.iter().map(ruby_ffi_type_src).collect::<PResult<_>>()?;
            (
                "Function",
                format!(
                    "::FFI::FunctionType.new({}, [{}])",
                    ruby_ffi_type_src(ret)?,
                    args.join(", ")
                ),
            )
        }
        Enum(_) | EnumSlot(_) => (
            "Mapped",
            format!("::FFI::Type::Mapped.new({})", builtin("INT32")),
        ),
        Str => ("String", builtin("STRING")),
        Pointer => ("Pointer", builtin("POINTER")),
        other => {
            let n = scalar_name(other)
                .ok_or_else(|| format!("FFI::Struct field type `{other:?}` has no descriptor"))?;
            ("Number", builtin(&n))
        }
    };
    Ok(format!(
        "::FFI::StructLayout::{class}.new(:{name}, {off}, {type_src}, {size}, {align}, self)"
    ))
}

fn ruby_ffi_type_src(ty: &crate::hir::FfiType) -> PResult<String> {
    use crate::hir::FfiType::*;
    Ok(match ty {
        Void => "::FFI::Type::VOID".to_string(),
        // Both enum tiers are an `int` on the wire; a callback trampoline
        // marshals through the raw value either way.
        Enum(_) | EnumSlot(_) => ":int32".to_string(),
        Callback(..) => ":pointer".to_string(),
        Struct(_) | Array(..) => {
            return Err(
                "a callback signature can't pass a struct BY VALUE (zeo limitation) -- \
                 use `.by_ref`"
                    .to_string()
                    .into(),
            );
        }
        // Synthesized source re-lowers in another body, where the alias
        // tables differ -- spell the typedef by its own name (it resolves
        // through the same `PLATFORM_TYPEDEFS` row) and reject the
        // return-only device.
        PlatformScalar(name) => format!(":{name}"),
        StrPtr => {
            return Err(
                "`:strptr` is only usable as an `attach_function` return type"
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
      def initialize(__p, __off, __n, __get, __put, __esize, __klass = nil)
        @__p, @__off, @__n, @__get, @__put, @__esize = __p, __off, __n, __get, __put, __esize
        @__klass = __klass
      end
      def size
        @__n
      end
      def [](__i)
        if @__klass
          @__klass.new(@__p + (@__off + __i * @__esize))
        elsif @__get.nil?
          raise ::ArgumentError, "get not supported for FFI::ArrayType"
        else
          @__p.send(@__get, @__off + __i * @__esize)
        end
      end
      def []=(__i, __v)
        if @__klass
          @__p.put_bytes(@__off + __i * @__esize, __v.to_ptr.get_bytes(0, @__esize))
        elsif @__put.nil?
          raise ::ArgumentError, "set not supported for FFI::ArrayType"
        else
          @__p.send(@__put, @__off + __i * @__esize, __v)
        end
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
        @__p + @__off
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
pub(crate) fn needs_inline_array_classes(fields: &[FfiField]) -> bool {
    fields
        .iter()
        .any(|(_, t, _)| matches!(t, crate::hir::FfiType::Array(..)))
}

/// The one place field offsets, total size and alignment are computed --
/// consumed by the accessor synthesis below AND recorded as
/// `FfiVocab::ffi_struct_layouts` for by-value passing, so the two views of the
/// same struct cannot disagree.
pub(crate) fn ffi_struct_layout(
    class_path: &str,
    fields: &[FfiField],
    union: bool,
) -> PResult<crate::hir::FfiStructLayout> {
    let round_up = |n: usize, a: usize| -> usize { n.div_ceil(a) * a };
    let mut offset = 0usize;
    let mut max_align = 1usize;
    // A union's members all start at offset 0 and it is as wide as its
    // widest member.
    let mut widest = 0usize;
    let mut placed = Vec::new();
    for (name, ty, pinned) in fields {
        let (_, _, size, align) = ffi_field_accessor(ty)?;
        // A PINNED offset wins outright -- the gem honours the number the
        // declaration gave, however it sits against the field's alignment.
        // Size and alignment still come from the fields themselves, which is
        // why `layout :a, :int16, 0, :b, :int32, 2, :c, :int16, 6` is 8 bytes
        // aligned to 4 and not 8 bytes aligned to 2 (oracle-verified).
        let off = match (union, pinned) {
            (true, _) => 0,
            (false, Some(off)) => *off,
            (false, None) => round_up(offset, align),
        };
        placed.push((name.clone(), ty.clone(), off));
        // The struct's extent, not the last field's end: pinned offsets need
        // not run in order.
        offset = offset.max(off + size);
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
        /// The deferred twin: the table lives in a runtime slot the class body
        /// filled, so the accessor calls through instead of carrying a
        /// literal. ethon's `layout :whatever, :pointer, :code, :easy_code`
        /// stores an enum whose members come from a method.
        EnumSlot(usize),
        Bool,
        Str,
        /// `(class, element count, element size, element struct class)` --
        /// the proxy the field reads back as. `FFI::StructLayout::CharArray`
        /// for an 8-bit element (it is the one that also answers `to_s`),
        /// `FFI::Struct::InlineArray` otherwise, matching the gem. A STRUCT
        /// element carries its class path: the proxy then constructs that
        /// class viewing each slot in place instead of a scalar getter.
        Array(&'static str, usize, usize, Option<String>),
        /// An inline array whose element is ITSELF an inline array -- X11's
        /// `layout :matrix, [[:XFixed, 3], 3]`. The layout is real (C lays a
        /// 2-D array out as rows of rows, and the following fields start
        /// after all of it), but the gem cannot read one back: indexing the
        /// proxy raises `ArgumentError: get not supported for FFI::ArrayType`,
        /// oracle-verified. `(class, element count, row size)`.
        NestedArray(&'static str, usize, usize),
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
            crate::hir::FfiType::EnumSlot(slot) => Conv::EnumSlot(*slot),
            crate::hir::FfiType::Bool => Conv::Bool,
            crate::hir::FfiType::Str => Conv::Str,
            crate::hir::FfiType::Array(elem, count)
                if matches!(**elem, crate::hir::FfiType::Array(..)) =>
            {
                let (_, _, esize, _) = ffi_field_accessor(elem)?;
                Conv::NestedArray("FFI::Struct::InlineArray", *count, esize)
            }
            crate::hir::FfiType::Array(elem, count) => {
                let (_, _, esize, _) = ffi_field_accessor(elem)?;
                let elem_class = match &**elem {
                    crate::hir::FfiType::Struct(l) if l.class_path.is_empty() => {
                        return Err(
                            "an inline array of structs needs a NAMED struct class (zeo limitation)"
                                .to_string()
                                .into(),
                        );
                    }
                    crate::hir::FfiType::Struct(l) => Some(l.class_path.clone()),
                    _ => None,
                };
                let class = if esize == 1 && elem_class.is_none() {
                    "FFI::StructLayout::CharArray"
                } else {
                    "FFI::Struct::InlineArray"
                };
                Conv::Array(class, *count, esize, elem_class)
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
    let alignment = layout.align;

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
                // A struct element passes its CLASS instead of a getter pair:
                // the proxy then views each slot in place.
                Conv::Array(class, count, esize, elem_class) => match elem_class {
                    Some(ec) => {
                        format!("{class}.new(@__ffi_ptr, {off}, {count}, nil, nil, {esize}, {ec})")
                    }
                    None => format!(
                        "{class}.new(@__ffi_ptr, {off}, {count}, :{getter}, :{putter}, {esize})"
                    ),
                },
                // No accessor pair at all: the proxy answers `size`/`to_ptr`
                // and raises on `[]`, which is the gem's own behaviour for a
                // row-of-rows element.
                Conv::NestedArray(class, count, esize) => {
                    format!("{class}.new(@__ffi_ptr, {off}, {count}, nil, nil, {esize})")
                }
                // The nested class VIEWING the field's bytes in place -- the
                // synthesized `initialize` takes the pointer as-is, so every
                // inner accessor indexes from parent + offset.
                Conv::Struct(class, _) => format!("{class}.new(@__ffi_ptr + {off})"),
                Conv::Callback(args, ret) => {
                    format!("::FFI::Function.new({ret}, [{args}], @__ffi_ptr.get_pointer({off}))")
                }
                Conv::Enum(m) => {
                    let table = m
                        .iter()
                        .map(|(n, v)| format!("{v} => :{n}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{{{table}}}.fetch({read}) {{ |__v| __v }}")
                }
                Conv::EnumSlot(slot) => format!("__zeo_ffi_enum_get({slot}, {read})"),
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
            if matches!(conv, Conv::Array(..) | Conv::NestedArray(..)) {
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
                Conv::Str
                | Conv::Array(..)
                | Conv::NestedArray(..)
                | Conv::Struct(..)
                | Conv::Callback(..) => unreachable!("returned above"),
                Conv::Enum(m) => {
                    let table = m
                        .iter()
                        .map(|(n, v)| format!("{n}: {v}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{{{table}}}.fetch(__ffi_value, __ffi_value)")
                }
                Conv::EnumSlot(slot) => format!("__zeo_ffi_enum_put({slot}, __ffi_value)"),
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
    let offsets: String = placed
        .iter()
        .map(|(name, _, _, off, _)| format!("[:{name}, {off}]"))
        .collect::<Vec<_>>()
        .join(", ");
    let descriptors: String = layout
        .fields
        .iter()
        .zip(&placed)
        .map(|((name, ty, off), _)| ffi_field_descriptor(name, ty, *off))
        .collect::<PResult<Vec<_>>>()?
        .join(",\n    ");

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
def self.alignment
  {alignment}
end
def self.offset_of(__ffi_field)
  case __ffi_field
{offset_arms}      else raise ArgumentError, "no such struct field #{{__ffi_field.inspect}}"
  end
end
def self.members
  [{members}]
end
def self.offsets
  [{offsets}]
end
def offsets
  self.class.offsets
end
def self.layout(*__ffi_args)
  unless __ffi_args.empty?
    raise ::NotImplementedError,
          "a `layout` this class body did not declare isn't supported yet (zeo limitation)"
  end
  @__zeo_layout ||= ::FFI::StructLayout.new([
    {descriptors}
  ], {total}, {alignment})
end
def layout
  self.class.layout
end
"#
    ))
}

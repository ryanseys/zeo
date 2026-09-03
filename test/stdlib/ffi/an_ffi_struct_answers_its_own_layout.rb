# A struct class describes itself: `.offsets`, and a `.layout` reader
# answering an `FFI::StructLayout` whose fields carry each field's name,
# offset, size, alignment and TYPE. zeo builds all of it from the layout its
# compiler already walked, so the descriptors describe exactly the bytes the
# generated accessors read.
#
# One divergence, stated in `ext/ffi/lib/ffi.rb` and not visible here: a
# `:long` field reports `Type::Builtin::INT64` rather than `LONG`, because
# the compiler folds the two to one width before a layout is recorded.
require "ffi"

class Inner < FFI::Struct
  layout :q, :int
end

class Wide < FFI::Struct
  layout :a, :int,
         :b, :pointer,
         :c, [:char, 8],
         :d, :double,
         :e, Inner,
         :f, :string,
         :g, callback([:int], :void),
         :h, :bool
end

p Wide.offsets
p Wide.new.offsets == Wide.offsets

l = Wide.layout
p [l.class, l.size, l.alignment, l.members]

# A layout IS a type, as are the three non-scalar descriptors -- so each
# carries the builtin type constants. Only `Field` is a plain object.
p [l.class.ancestors.first(3), l.is_a?(FFI::Type)]
p [FFI::ArrayType.superclass, FFI::StructByValue.superclass, FFI::FunctionType.superclass]
p [FFI::StructLayout::Field.superclass, FFI::StructLayout.const_get(:INT32).inspect]
l.fields.each { |f| p [f.class, f.name, f.offset, f.size, f.alignment, f.type.class] }

# The reader is memoized, and `to_a` is `fields`.
p [Wide.layout.equal?(Wide.layout), l.to_a == l.fields]
p [l.offset_of(:d), l[:e].offset, l.offsets == Wide.offsets]

# An inline array's type knows its element and its count.
p [l[:c].type.length, l[:c].type.elem_type.inspect]

# A field reads and writes through a pointer to the struct's first byte,
# with every conversion the struct's own `[]` performs.
s = Wide.new
s[:a] = 7
s[:d] = 1.5
p [l[:a].get(s.pointer), l[:d].get(s.pointer)]
l[:a].put(s.pointer, 9)
p s[:a]
p l[:c].get(s.pointer).class

# A name this layout has no field for answers nil, not an error.
p l[:nope]
__END__
[[:a, 0], [:b, 8], [:c, 16], [:d, 24], [:e, 32], [:f, 40], [:g, 48], [:h, 56]]
true
[FFI::StructLayout, 64, 8, [:a, :b, :c, :d, :e, :f, :g, :h]]
[[FFI::StructLayout, FFI::Type, Object], true]
[FFI::Type, FFI::Type, FFI::Type]
[Object, "#<FFI::Type::Builtin::INT32 size=4 alignment=4>"]
[FFI::StructLayout::Number, :a, 0, 4, 4, FFI::Type::Builtin]
[FFI::StructLayout::Pointer, :b, 8, 8, 8, FFI::Type::Builtin]
[FFI::StructLayout::Array, :c, 16, 8, 1, FFI::ArrayType]
[FFI::StructLayout::Number, :d, 24, 8, 8, FFI::Type::Builtin]
[FFI::StructLayout::InnerStruct, :e, 32, 4, 4, FFI::StructByValue]
[FFI::StructLayout::String, :f, 40, 8, 8, FFI::Type::Builtin]
[FFI::StructLayout::Function, :g, 48, 8, 8, FFI::FunctionType]
[FFI::StructLayout::Number, :h, 56, 1, 1, FFI::Type::Builtin]
[true, true]
[24, 32, true]
[8, "#<FFI::Type::Builtin::INT8 size=1 alignment=1>"]
[7, 1.5]
9
FFI::StructLayout::CharArray
nil

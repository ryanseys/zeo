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

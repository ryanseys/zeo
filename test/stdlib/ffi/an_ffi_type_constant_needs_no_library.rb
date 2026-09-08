# `FFI::Type::X` is an OBJECT, and ruby-ffi's `find_type` reads whatever
# value the constant holds. So the vocabulary a struct spends may sit in a
# plain namespace, and a layout may name the type object outright -- neither
# needs a module that `extend FFI::Library`.
require "ffi"

module Vocab
  WIDE = FFI::Type::UINT16
  TALL = FFI::Type::UINT32

  class Cell < FFI::Struct
    layout :wide, WIDE,
           :tall, TALL,
           :deep, FFI::Type::UINT64
  end
end

p Vocab::Cell.size
p [Vocab::Cell.offset_of(:wide), Vocab::Cell.offset_of(:tall), Vocab::Cell.offset_of(:deep)]

cell = Vocab::Cell.new
cell[:wide] = 65_535
cell[:tall] = 4_294_967_295
cell[:deep] = 42
p [cell[:wide], cell[:tall], cell[:deep]]
__END__
16
[0, 4, 8]
[65535, 4294967295, 42]

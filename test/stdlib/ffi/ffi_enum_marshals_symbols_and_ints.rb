# FFI `enum`: a named `enum :tag, [...]` used as an
# `attach_function` type. A Symbol argument marshals to its int; an int return
# maps back to its Symbol (an unmapped int stays an Integer). Auto-increment
# after an explicit value (`:next` = 101). Verified against `ffi 1.17.4` using
# `labs` as a pass-through: `labs(:hundred)` sends 100, `labs(-100)` returns
# 100 -> `:hundred`.

require "ffi"
module C
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  enum :nums, [:zero, 0, :hundred, 100, :next]
  attach_function :to_num, :labs, [:nums], :long
  attach_function :from_num, :labs, [:long], :nums
end
puts C.to_num(:hundred)
puts C.from_num(-100).inspect
puts C.from_num(-101).inspect
puts C.from_num(-5).inspect
__END__
100
:hundred
:next
5

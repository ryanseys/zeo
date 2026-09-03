# FFI `typedef :existing, :alias`: a library-local type alias,
# declared before use as the gem requires, resolves in a later
# `attach_function`'s type list. Behaves identically to naming the underlying
# type -- a pure compile-time aliasing, matching the gem.

require "ffi"
module L
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  typedef :int, :myint
  typedef :myint, :myint2
  attach_function :abs, [:myint2], :myint
end
puts L.abs(-5)
__END__
5

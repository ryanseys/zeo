# An FFI integer argument given a non-Integer is a `TypeError`, exactly as the
# gem raises -- the marshaling is faithful, not a silent coercion.

require "ffi"
module L
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :abs, [:int], :int
end
begin
  L.abs("not an int")
  puts "no error"
rescue TypeError
  puts "TypeError"
end
__END__
TypeError

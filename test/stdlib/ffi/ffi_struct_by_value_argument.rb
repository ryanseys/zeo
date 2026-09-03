# `.by_value` is the gem's BY-VALUE spelling: `inet_ntoa` takes `struct
# in_addr` by value on every libc, and the repr(C) mirror carries the real
# field types so rustc owns the ABI.
#
# A BARE `FFI::Struct` subclass in the same position is the gem's
# `StructByReference` -- it passes a POINTER (oracle-checked: ruby-ffi hands
# `inet_ntoa` the struct's address and gets an address-shaped dotted quad
# back). `gettimeofday` is the honest way to show it, since a C function that
# really wants the pointer then works.

require "ffi"
class InAddr < FFI::Struct
  layout s_addr: :uint32
end
class Timeval < FFI::Struct
  layout :tv_sec, :long, :tv_usec, :long
end
module L
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :inet_ntoa, [InAddr.by_value], :string
  attach_function :gettimeofday, [Timeval, :pointer], :int
end
a = InAddr.new
a[:s_addr] = 16777343
puts L.inet_ntoa(a)
tv = Timeval.new
puts L.gettimeofday(tv, nil)
puts tv[:tv_sec] > 1_600_000_000
__END__
127.0.0.1
0
true

# The direct tier: every C scalar kind in and out, through the emitted
# `call_indirect` rather than the libffi engine. Widths and signedness
# are what the test is about -- a `long` past 32 bits, an `unsigned long`
# past `i64::MAX` (a Bignum), a `float` demoted and promoted, a `:bool`
# read from an `int` result's low byte (`access` answers 0 or -1, so both
# libcs agree; glibc's `isalpha` answers 1024, whose low byte is 0),
# a NULL `:string` result (`nil`), a `:pointer` result read back.

require "ffi"
module LibC
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :abs, [:int], :int
  attach_function :labs, [:long], :long
  attach_function :my_strlen, :strlen, [:string], :ulong
  attach_function :getenv, [:string], :string
  attach_function :strdup, [:string], :pointer
  attach_function :free, [:pointer], :void
  attach_function :strtoul, [:string, :pointer, :int], :ulong
  attach_function :access, [:string, :int], :bool
  attach_function :toupper, [:int], :int
end
module LibM
  extend FFI::Library
  ffi_lib "m"
  attach_function :fabs, [:double], :double
  attach_function :fabsf, [:float], :float
end
p LibC.abs(-7), LibC.labs(-(2**40)), LibC.my_strlen("hello")
p LibC.getenv("ZEO_NO_SUCH_VARIABLE")
d = LibC.strdup("copied")
p d.read_string
p LibC.free(d)
p LibC.strtoul("18446744073709551615", nil, 10)
p LibC.access("/no/such/path", 0), LibC.access("/", 0), LibC.toupper("a".ord).chr
p LibM.fabs(-2.5), LibM.fabsf(-1.5)
__END__
7
1099511627776
5
nil
"copied"
nil
18446744073709551615
true
false
"A"
2.5
1.5

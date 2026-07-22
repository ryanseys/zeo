# Variadic FFI through the real ffi gem API: a trailing :varargs spec makes
# the function variadic, and each call passes (type, value) pairs for the
# variable part with C default promotions (int->int, float->double,
# str->const char*). NOTE: printf writes through C stdio, whose buffer
# flushes after Ruby's own stdout at process exit, so when stdout is a pipe
# all printf output trails the Ruby-side puts output.
require "ffi"

module C
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :printf, [:string, :varargs], :int
end

# int + int + str varargs; printf returns the byte count written
n = C.printf("%d-%d-%s\n", :int, 1, :int, 22, :string, "hi")
puts n

# float vararg promotes to double
C.printf("%.2f\n", :double, 3.5)

# multiple mixed args
C.printf("[%s=%d]\n", :string, "k", :int, 7)

# no extra args (just the fixed format)
C.printf("plain\n")

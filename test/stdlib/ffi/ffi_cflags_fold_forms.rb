# Ported from spinel's ffi_cflags test to the real ffi gem API. ffi_cflags
# has no gem analogue (the gem links against built libraries, it doesn't
# compile C), so the surviving intent is the multi-module shape it decorated:
# two library modules each bind their own functions out of libm.
require "ffi"

module MathA
  extend FFI::Library
  ffi_lib "m"
  attach_function :fabs, [:double], :double
end

module MathB
  extend FFI::Library
  ffi_lib "m"
  attach_function :cos, [:double], :double
end

puts MathA.fabs(-1.5)
puts MathB.cos(0.0)
__END__
1.5
1.0

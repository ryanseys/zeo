# The real `ffi` gem API, AOT-compiled: `require "ffi"` is a native
# no-op, `extend FFI::Library` marks the module, and `attach_function` (plain
# and the 4-arg rename form) emits a compile-time `extern "C"` + `#[link]` and
# a wrapper module method. Scalar marshaling (`:int`/`:string`/`:ulong`/
# `:double`) is byte-identical to CRuby+ffi -- the whole point of the
# CRuby-faithful surface. libc/libm are always present, so this runs anywhere.

require "ffi"
module LibC
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :abs, [:int], :int
  attach_function :my_strlen, :strlen, [:string], :ulong
end
module LibM
  extend FFI::Library
  ffi_lib "m"
  attach_function :pow, [:double, :double], :double
end
puts LibC.abs(-7)
puts LibC.my_strlen("hello world")
puts LibM.pow(2.0, 10.0)
__END__
7
11
1024.0

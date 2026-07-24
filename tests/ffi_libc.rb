# Real `ffi` gem API, AOT-compiled (#204): `attach_function` emits a
# compile-time `extern "C"` + `#[link]`; the program runs identically under
# CRuby+ffi and zeo.
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

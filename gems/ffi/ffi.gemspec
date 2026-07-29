# The Ruby half of `ffi`. Its native half is zeo's own FFI: the compile-time
# `attach_function` frontend plus the statically linked `ext-ffi` runtime tier
# (Pointer/MemoryPointer, Type, DynamicLibrary, Function, VariadicInvoker).
# This half holds only what belongs in Ruby: the gem's exception classes and
# the small `Platform`/`DataConverter` mixins. Verified against ffi 1.17.4.
Gem::Specification.new do |s|
  s.name = "ffi"
  s.version = "1.17.4"
  s.summary = "Ruby FFI, AOT-compiled: the ffi gem API over zeo's native FFI."
  s.require_paths = ["lib"]
end

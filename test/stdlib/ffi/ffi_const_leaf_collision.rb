# A constant in an FFI-library module whose leaf name collides with a plain
# constant in another module must resolve parent-qualified (a leaf-keyed
# constant table would silently rebind the reference and its type). Ported
# from spinel's ffi_const to the real ffi gem API: the constant is a plain
# Ruby constant on the FFI::Library module.
require "ffi"

module Verbs
  TEXT = "download-text"
end

module CMath
  extend FFI::Library
  ffi_lib "m"
  TEXT = 3
  attach_function :fabs, [:double], :double
end

t = 3
puts(t == CMath::TEXT ? "int" : "collision")
puts Verbs::TEXT
puts CMath::TEXT + 1
puts CMath.fabs(-2.5)
__END__
int
download-text
4
2.5

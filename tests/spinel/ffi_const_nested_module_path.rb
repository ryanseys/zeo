# A constant in an FFI-library module referenced through a NESTED constant
# path (Outer::CMath::MODE) must still resolve parent-qualified by the
# module's leaf name -- a leaf-keyed plain-constant table would otherwise
# claim the reference for a same-leaf constant in another module. Ported from
# spinel's ffi_const to the real ffi gem API: the constant is a plain Ruby
# constant on the FFI::Library module.
require "ffi"

module Verbs
  MODE = "verbose"
end

module Outer
  module CMath
    extend FFI::Library
    ffi_lib "m"
    MODE = 7
    attach_function :fabs, [:double], :double
  end
end

m = 7
puts(m == Outer::CMath::MODE ? "int" : "collision")
puts Verbs::MODE
puts Outer::CMath::MODE + 1
puts Outer::CMath.fabs(-1.5)

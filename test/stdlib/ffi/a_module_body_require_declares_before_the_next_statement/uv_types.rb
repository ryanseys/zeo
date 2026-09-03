# The sibling vocabulary file: reopens the library module and declares the
# typedef/enum the requiring body's very next statements spend.
require "ffi"

module Loop
  extend FFI::Library

  typedef :long, :uv_ret
  enum :uv_kind, [:tcp, 3, :udp, 7]
end

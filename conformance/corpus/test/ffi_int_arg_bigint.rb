# An FFI :int argument fed by an integer computed through repeated
# multiplication (a value a widening analysis could have promoted) still
# marshals as a plain C int. Ported from spinel's ffi_func to the real ffi
# gem API.
require "ffi"

module M
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :abs, [:int], :int
end
class P
  def initialize
    @mult = 2
    @base = 5
  end
  def backoff_for(n)
    d = @base
    i = 0
    while i < n
      d = d * @mult
      i += 1
    end
    d
  end
end
p = P.new
b = p.backoff_for(3)   # 5*2*2*2 = 40
puts M.abs(b)

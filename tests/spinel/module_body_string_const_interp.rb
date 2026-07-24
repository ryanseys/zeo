# Interpolating a String constant defined in the same module/class body
# must read the constant's value, not its raw pointer. Before, the
# constant reference inside another constant's initializer was annotated
# as Integer (the lexical scope was not set while caching the RHS), so
# "v#{A}" emitted the pointer of A formatted as %lld.

module M
  A = "33"
  V = "v#{A}"
end
puts M::V

class C
  NAME = "spinel"
  GREETING = "hello #{NAME}"
end
puts C::GREETING

# Chained-VERSION shape (e.g. libdatadog's VERSION = "#{LIB}.#{MAJOR}.#{MINOR}")
module Lib
  LIB = "ddtrace"
  MAJOR = "1"
  MINOR = "2"
  VERSION = "#{LIB}-#{MAJOR}.#{MINOR}"
end
puts Lib::VERSION

# Constant-to-constant value reference inside a body
module V
  A = "hi"
  B = A
end
puts V::B

# Control: an Integer constant interpolates as its number
module I
  N = 33
  S = "n=#{N}"
end
puts I::S

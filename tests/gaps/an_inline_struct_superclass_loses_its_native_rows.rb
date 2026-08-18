# `class T < Struct.new(:b)` -- an INLINE anonymous struct as the superclass.
# The runtime ancestry is right (`T.ancestors` shows the anonymous class, then
# `Struct`), but the COMPILE-time chain for T does not reach `Struct`, so
# `mro::materialize_methods` never sees Struct's native rows and a
# `module Enumerable` reopen claims `size` for T. The named spelling
# (`S = Struct.new(:b); class T < S; end`) resolves and answers correctly.
module Enumerable
  def size
    :enum_size
  end
end

class T < Struct.new(:b)
end

p T.new(2).size
p T.instance_method(:size).owner.to_s

S = Struct.new(:c)
class U < S
end
p U.new(3).size

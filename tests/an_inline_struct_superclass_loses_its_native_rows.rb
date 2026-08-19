# `class T < Struct.new(:b)` -- an INLINE anonymous struct as the superclass.
# The class is minted at RUNTIME, and a runtime class's dispatch walk probed
# each ancestor's overlay, registry and value rows but never its NATIVE table,
# which is the only place a builtin's own rows live. So a `module Enumerable`
# reopen answered `size` from two positions FARTHER out than `Struct#size`.
#
# The named spelling (`S = Struct.new(:b); class T < S; end`) compiles to a
# real class and never took that walk.
module Enumerable
  def size
    :enum_size
  end

  def only_enum
    :enum_only
  end
end

class T < Struct.new(:b)
end

p T.new(2).size
p T.new(2).only_enum
p T.instance_method(:size).owner.to_s
p T.ancestors.map { |c| c.name.inspect }

S = Struct.new(:c)
class U < S
end
p U.new(3).size

# A runtime class's OWN definition still wins over the nearer builtin row.
K = Class.new(Struct.new(:d)) do
  def size = :own
end
p K.new(1).size

# A runtime subclass of a value-backed builtin reaches its rows THROUGH the
# payload -- handing the row a boxed object instead panicked its downcast.
A2 = Class.new(Array)
p [A2.new([1, 2, 3]).size, A2.new([1, 2]).first, A2.new([1]).class == A2]
S3 = Class.new(String)
p [S3.new("abc").length, S3.new("abc").upcase]
H2 = Class.new(Hash)
p H2.new.size

# ...and a module included at RUNTIME still layers above them.
module Louder
  def upcase = :louder
end
S4 = Class.new(String)
S4.include(Louder)
p S4.new("x").upcase

# `super` from a runtime class reaches the builtin row.
S5 = Class.new(String) do
  def upcase = "<#{super}>"
end
p S5.new("hi").upcase

# Redefining an already-compiled method AT RUNTIME has to take effect.
#
# A runtime definition lands in the overlay, and only DYNAMIC dispatch consults
# it -- so a call whose receiver class zeo knows at compile time keeps answering
# with the compiled body. Path 1 therefore de-optimizes any name a runtime
# definition site could install (`Compiler::runtime_redefs`), the same way a
# guarded `undef_method` already de-optimizes its own.
#
# The literal-name form inside a class body is untouched: lowering desugars it
# into an ordinary `def` before any of this, so no call survives to collect.

class Node
  attr_accessor :value

  def initialize(v)
    @value = v
  end
end

n = Node.new(7)
p n.value

# A COMPUTED name, so the literal-name `define_method` lowering (which zeo
# compiles as an ordinary `def`) cannot see it.
name = [:va, :lue].join.to_sym
Node.send(:define_method, name) { "dyn-#{@value}" }
p [n.value, n.send(:value), Node.new(1).value]

m = Node.new(5)
m.define_singleton_method(:value) { "sing" }
p [m.value, m.send(:value), n.value]

class Other
  attr_reader :z

  def initialize
    @z = 1
  end
end

o = Other.new
def o.z
  "obj-singleton"
end
p [o.z, Other.new.z]

# A LITERAL name still counts once the call reaches runtime -- with an explicit
# receiver, or through `send`, neither of which desugars.
class Plain
  def a = "a0"
  def b = "b0"
  def c = "c0"
end
q = Plain.new
Plain.define_method(:a) { "a1" }
p [q.a, Plain.new.a]
Plain.send(:define_method, :b) { "b1" }
p q.b

# A runtime `alias_method` replaces the target the same way.
Plain.send(:alias_method, :c, :a)
p q.c

# The class-body form keeps its compile-time lowering, so the LAST definition
# in the body wins and nothing de-optimizes.
class Body
  def w = "first"
  define_method(:w) { "second" }
end
p Body.new.w

# Redefining under a name nothing else uses leaves the rest of the program
# alone: `value` above is dynamic now, `size` here is not.
class Sized
  def size = 3
end
p Sized.new.size

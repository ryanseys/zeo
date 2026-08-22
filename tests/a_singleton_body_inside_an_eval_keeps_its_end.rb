# A class body inside an `eval` runs as one more `class_eval` of its own
# SOURCE TEXT, recovered by slicing the snippet -- and the slice runs to the
# CLASS's own `end`, not to its last statement.
#
# `class << self` is not a statement: `lower::defs` SPLICES it into the
# enclosing class body as an empty surrogate reopen carrying the keyword's
# span, then the retagged `def`s carrying theirs. So the last statement stops
# short of the `end` that closes the singleton body, and prism reported an
# unterminated `class`.
#
# The class node's own span ends past its `end` keyword, which is the one
# thing that says where the body really stops. Checked before it is used --
# the three bytes really are `end`, and they really are past the last
# statement -- so a body whose statements analyze REWROTE (an `FFI::Struct`'s
# synthesized accessors) still takes the other path.

eval <<~SRC
  class SelfSing
    class << self
      def hi = "class method"
    end
    def inst = "instance"
  end
SRC
p [SelfSing.hi, SelfSing.new.inst]

# An `attr_accessor` in the singleton body, which is what the shape is
# normally written for.
eval <<~SRC
  class Configured
    def a = 1
    class << self
      attr_accessor :cfg
    end
  end
SRC
Configured.cfg = 5
p [Configured.cfg, Configured.new.a]

# `class << obj` was never affected, and neither was a module's.
o = Object.new
eval <<~SRC
  class << o
    def only = "obj"
  end
SRC
p o.only

eval <<~SRC
  module Meta
    class << self
      def mm = "module meta"
    end
  end
SRC
p Meta.mm

# An ordinary body still slices to the same place.
eval <<~SRC
  class Plain
    A = 1
    def b = A
  end
SRC
p Plain.new.b

# `@@x` resolves against the LEXICAL cref chain, and only `class`/`module`
# bodies open one. Outside any of them Ruby raises rather than storing.

class Counted
  @@count = 0
  def self.bump = @@count += 1
  def count = @@count
end
Counted.bump
p Counted.new.count
p Counted.class_variables

# A `def` and a block both inherit the enclosing cref rather than opening one.
class Holder
  define_method(:set) { @@via_block = :yes }
  def self.read = @@via_block
end
Holder.new.set
p Holder.read

# Reopening Object explicitly IS a cref, so this stores rather than raising.
class Object
  @@on_object = 1
end
p Object.class_variables.include?(:@@on_object)

# `class << self` is skipped when resolving, so this lands on Holder.
class Holder
  class << self
    @@from_singleton = 7
  end
end
p Holder.class_variables.sort

def raised
  yield
  "no raise"
rescue RuntimeError => e
  e.message
end

p raised { @@toplevel_read }
p raised { @@toplevel_write = 1 }
p raised { @@toplevel_op ||= 1 }
p raised { @@a, @@b = 1, 2 }
p raised { Class.new { @@in_class_new = 1 } }
p raised { Module.new { @@in_module_new = 1 } }
p raised { Holder.class_eval { @@in_class_eval = 1 } }

# `defined?` never evaluates its operand, so it answers nil instead.
p defined?(@@never_assigned)

# The right-hand side has already run by the time the raise fires.
side_effect = nil
p raised { @@late = (side_effect = :ran) }
p side_effect

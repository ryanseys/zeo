# A core class object answers the Class/Module surface every class has, but it
# has no user class entry, so the fold never consulted that list and the probe
# cannot type a bare `Range.new` -- it is the call that builds a value, not one
# that has a value (#3494).
p Range.new(1, 3).to_a
p Range.respond_to?(:new)
p Symbol.respond_to?(:all_symbols)
p Array.respond_to?(:instance_method)
p Array.respond_to?(:new)
p Array.respond_to?(:instance_methods)
p String.respond_to?(:ancestors)
p Integer.respond_to?(:superclass)
p Hash.respond_to?(:method_defined?)

# a name a class object genuinely does not answer stays false
p Array.respond_to?(:no_such_class_method)
p Range.respond_to?(:all_symbols)

# a module answers the module surface but not :new
module Mod
  def helper
    1
  end
end
p Mod.respond_to?(:instance_methods)
p Mod.respond_to?(:instance_method)
p Mod.respond_to?(:new)

# a user class still answers its own class methods and the universal ones
class UC
  def self.build
    new
  end
end
p UC.respond_to?(:build)
p UC.respond_to?(:new)
p UC.respond_to?(:instance_method)
p UC.respond_to?(:nope)

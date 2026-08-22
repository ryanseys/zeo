# A module both INCLUDED and PREPENDED into a class's SINGLETON class holds
# two positions there, exactly as it does in an instance ancestry -- and its
# body runs once per position.
#
# It took both halves of the class-method channel, and the second is the one
# that made this a project. The CHAIN had to hold the duplicate: its two areas
# dedup apart, because `rb_include_module` searches the whole chain and
# `rb_prepend_module` only the prepend area. And DISPATCH had to have
# positions to walk, which it did not: a class method resolved through
# `send_super_class_from`'s three branches over `class_method_prepends`, the
# extend sources and the flattened `class_methods` tables, never over an
# ancestry. There was nothing there to hold a module twice, so the second copy
# re-entered the first and P2 below died of SystemStackError.
#
# `dispatch::singleton_walk` is that ancestry now, derived per walk: for each
# non-module ancestor, its singleton prepends, its own `def self.x` layer, then
# its extends. `super` resumes past the POSITION the running body occupies --
# published in a resume cell of its own, since the instance channel indexes a
# different sequence and a module used both ways would collide with it.
#
# Two things about that position are not obvious and both cost a debugging
# pass. A materialized copy of an extended module's method bakes the class it
# was materialized ONTO, not the module, so the position it occupies is the
# first one at or after that class's layer which really answers. And a `super`
# the COMPILER resolved enters a body the walk also knows a position for, so
# it has to publish the same one or the walk reaches that body again.
#
# The wider sweep is `tests/singleton_chain_dispatch_sweep.rb`.

module CM
  def hi = "cm(#{defined?(super) ? super : 'top'})"
end

class P1
  extend CM
  singleton_class.prepend CM
  def self.hi = "own(#{defined?(super) ? super : 'top'})"
end
p P1.singleton_class.ancestors.map(&:to_s)
p P1.hi

class P2
  singleton_class.include CM
  singleton_class.prepend CM
end
p P2.singleton_class.ancestors.map(&:to_s)
p P2.hi

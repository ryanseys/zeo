# `defined?` on a TOP-LEVEL constant a compiled-in unit assigns, read from
# inside a class body, answers "constant".
#
# A constant whose only assignment lives in a unit cannot be folded -- the
# unit has not run -- so the read becomes a runtime probe, and the probe is
# `Search::Scoped`, which deliberately excludes `Object`-owned constants. The
# owner the emitter guessed was the enclosing class, so a unit's top-level
# constant was invisible to `defined?` while the READ beside it answered the
# value: a silent wrong answer, and one that widens with every unit a program
# compiles in, because the set is keyed by BARE LEAF name.
#
# `Object` is probed too, which is the other half of ruby's own rule for a
# bare constant rather than a widening of it.

require_relative "a_unit_toplevel_constant_is_defined_from_a_class_body/latecomer" unless defined?(Latecomer)

class Reader
  def top_defined = defined?(TOP_MARK)
  def top_value = TOP_MARK
  def scoped_defined = defined?(Latecomer::INSIDE)
  def missing_defined = defined?(NEVER_ASSIGNED_ANYWHERE)
end

r = Reader.new
p r.top_defined
p r.top_value
p r.scoped_defined
p r.missing_defined
# ...and at the top level, where the owner guess was already Object.
p defined?(TOP_MARK)
p defined?(Latecomer::INSIDE)

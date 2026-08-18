# `Later.define_singleton_method(:mode)` must replace the compiled
# `def self.mode` only from the point it RUNS. zeo answers `:runtime` for the
# call written before it, so the redefinition reaches backwards in time.
#
# Pre-existing, and unrelated to class-method call-site caching: the same two
# lines diverge identically with every `Target.name` site uncached. The
# `overlay_class_method` probe in `send_value_in_reason` is gated on
# `is_live()`, which is a WHOLE-PROGRAM flag -- once anything is defined at
# runtime it is on for every call, including ones that already happened in
# source order. The instance side has the same shape.
#
# The sibling shape that DOES work is `def Counter.tick = 2`, a top-level
# singleton def, which is registered where it is written -- see
# `tests/a_class_method_site_stays_correct_when_the_class_changes.rb`.
class Later
  def self.mode = :compiled
end
p Later.mode
Later.define_singleton_method(:mode) { :runtime }
p Later.mode

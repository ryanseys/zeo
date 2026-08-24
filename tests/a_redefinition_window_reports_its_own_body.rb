# Inside a REDEFINITION WINDOW -- between two same-name `def`s -- reflection
# answers for the body that is LIVE, not the last one written.
#
# `analyze::redefs` put the timeline in the runtime overlay: the first body
# installs at boot and each later one re-installs at its own document
# position. What it did NOT carry was everything beside the body. `#arity`,
# `#parameters` and `#source_location` are compile-time tables keyed by
# `(class, name)` with no position, so from the program's first line they
# answered the final `def`.
#
# The fix is a reflection row per BODY (`ProgramDesc::redef_metas`), seeded
# UNregistered and installed by whichever position is live -- the boot install
# for the first body, the positional install for each later one. Both halves
# of a `def` now land at the same place.
#
# Still open, and filed: a `private` mark made before a reopen is not enforced
# in the window (a_redefinition_window_ignores_a_visibility_mark.rb), and a
# `Method`/`UnboundMethod` captured in the window does not snapshot its
# definition (a_method_object_does_not_snapshot_its_definition.rb).

def loc(m) = [m.arity, m.parameters, m.source_location&.last]

class R
  def r = "r1"
end
p loc(R.instance_method(:r))
class R
  def r(x, y = 1, *z, k:, **o, &b) = "r2"
end
p loc(R.instance_method(:r))
class R
  def r(a) = "r3"
end
p loc(R.instance_method(:r))
p R.new.r(1)

class CR
  def self.c = "c1"
end
p loc(CR.method(:c))
class CR
  def self.c(x) = "c2"
end
p loc(CR.method(:c))

# three bodies in ONE body, with statements between
class T
  def t = 1
  p loc(instance_method(:t))
  def t(a) = 2
  p loc(instance_method(:t))
  def t(a, b) = 3
  p loc(instance_method(:t))
end

# a method_added hook observing each
class H
  def self.method_added(n)
    p [:added, n, instance_method(n).arity] unless n == :method_added
  end
  def h = 1
  def h(a) = 2
end

# a module's method
module MM
  def m = 1
end
p loc(MM.instance_method(:m))
module MM
  def m(a, b) = 2
end
p loc(MM.instance_method(:m))

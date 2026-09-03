# A definition on a module is inherited by every class that includes it, and
# zeo emits its body ONCE rather than once per includer. This pins what that
# makes observable.
#
# The body may not depend on the carrier. A constant it reads must resolve
# lexically from the MODULE, `self` must be the receiver, a method it calls
# must dispatch to the receiver's own override, `super` must resume from the
# module's position in the CARRIER's ancestry, and the private visibility a
# module gives a method must survive on every includer.
#
# A body that names an instance variable is deliberately NOT shared: an
# includer reads ivars by slot, and two includers can lay their slots out
# differently. Those rows are here so the split stays visible.

module Named
  IN_MODULE = "from-Named"

  def lit = 42
  def const_read = IN_MODULE
  def who = self.class.name
  def calls_out = "via:#{tag}"
  def with_block = [1, 2].map { |z| z * 2 }
  def stashes = (@stashed ||= 0) + 1

  private

  def secret = "shh"

  public

  def reaches_private = secret
end

# A SECOND module defining the same names: sharing is keyed by the definition,
# never by the method name, and these must not collide.
module Other
  def lit = "other-lit"
  def who = "other-who"
end

class Base
  def tag = "base"
end

class Plain < Base
  include Named
end

class Overrides < Base
  include Named
  def tag = "overrides"
end

class Elsewhere < Base
  include Other
end

class Supering < Base
  include Named
  def with_block = ["S"] + super
end

# A carrier with its OWN ivars, so the shared body's `self` is a receiver
# whose layout differs from the next one's.
class Roomy < Base
  include Named
  def initialize
    @a = 1
    @b = 2
  end
end

plain = Plain.new
over = Overrides.new
roomy = Roomy.new

p [plain.lit, over.lit, Elsewhere.new.lit]
p [plain.const_read, roomy.const_read]
p [plain.who, over.who, Elsewhere.new.who]
p [plain.calls_out, over.calls_out]
p [plain.with_block, Supering.new.with_block]

# The ivar a shared-eligible body would touch belongs to the RECEIVER, and
# each receiver keeps its own.
p [plain.stashes, roomy.stashes]
p [plain.instance_variables.sort, roomy.instance_variables.sort]

# A module's `private` reaches every includer.
p [plain.respond_to?(:secret), over.respond_to?(:secret)]
p [plain.reaches_private, over.reaches_private]
p [plain.send(:secret), roomy.send(:secret)]

# Reflection names the module, not the carrier.
p [plain.method(:lit).owner, Overrides.instance_method(:lit).owner]
p [Elsewhere.instance_method(:lit).owner, plain.is_a?(Named)]
p [Plain.include?(Named), Elsewhere.include?(Named)]
__END__
[42, 42, "other-lit"]
["from-Named", "from-Named"]
["Plain", "Overrides", "other-who"]
["via:base", "via:overrides"]
[[2, 4], ["S", 2, 4]]
[1, 1]
[[:@stashed], [:@a, :@b, :@stashed]]
[false, false]
["shh", "shh"]
["shh", "shh"]
[Named, Named]
[Other, true]
[true, false]

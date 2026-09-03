# A definition on a superclass is inherited by every subclass, and zeo emits
# its body ONCE rather than once per subclass. This pins what that makes
# observable -- the class half of the module rule beside it.
#
# The body may not depend on the carrier. A constant it reads resolves from
# the DEFINING class, `self` is the receiver, a method it calls dispatches to
# the receiver's own override, `super` resumes from the defining class's
# position, and the frame names the class the `def` was written in.
#
# A body that names an instance variable is deliberately NOT shared: a
# carrier reads ivars by slot. Those rows are here so the split stays
# visible.

class Base
  LIMIT = 10

  def plain = "base-plain"
  def reads_const = LIMIT
  def selfy = self.class.name
  def calls_hook = "calls:#{hook}"
  def hook = "base-hook"
  def with_block = [1, 2].map { |z| z * 2 }
  def stashes = (@count ||= 0) + 1

  private

  def secret = "quiet"

  public

  def reaches_private = secret
end

class Mid < Base
  def hook = "mid-hook"
end

class Leaf < Mid
  def with_block = ["L"] + super
end

# A carrier with its own ivars, so the shared body's receiver has a layout
# the defining class never saw.
class Roomy < Base
  def initialize
    @a = 1
    @b = 2
  end
end

base, mid, leaf, roomy = Base.new, Mid.new, Leaf.new, Roomy.new

p [base.plain, mid.plain, leaf.plain, roomy.plain]
p [base.reads_const, leaf.reads_const]
p [base.selfy, mid.selfy, leaf.selfy, roomy.selfy]

# The call inside a shared body still finds the RECEIVER's override.
p [base.calls_hook, mid.calls_hook, leaf.calls_hook]
p [base.with_block, mid.with_block, leaf.with_block]

# An ivar a body touches belongs to the receiver, and each keeps its own.
p [base.stashes, roomy.stashes]
p [base.instance_variables.sort, roomy.instance_variables.sort]

# `private` on the superclass reaches every subclass.
p [leaf.respond_to?(:secret), leaf.reaches_private, roomy.send(:secret)]

# Reflection names the defining class, not the carrier.
p [leaf.method(:plain).owner, Roomy.instance_method(:plain).owner]
p [Leaf.instance_method(:hook).owner, Leaf.superclass, Mid.superclass]

# The frame does too -- three classes deep.
begin
  leaf.reads_const
  raise "x" if leaf.plain
rescue RuntimeError
  # `plain` is Base's, so a raise INSIDE it would say Base#plain; this only
  # pins that the shared body ran at all.
end
p Leaf.ancestors.first(3)
__END__
["base-plain", "base-plain", "base-plain", "base-plain"]
[10, 10]
["Base", "Mid", "Leaf", "Roomy"]
["calls:base-hook", "calls:mid-hook", "calls:mid-hook"]
[[2, 4], [2, 4], ["L", 2, 4]]
[1, 1]
[[:@count], [:@a, :@b, :@count]]
[false, "quiet", "quiet"]
[Base, Base]
[Mid, Mid, Base]
[Leaf, Mid, Base]

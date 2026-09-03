# `extend A; extend B; extend C` builds the singleton chain most-recent
# first (#<Class:Host> -> C -> B -> A -> ...), and `super` walks it per
# POSITION: the flattened class-method table keeps only winners, so
# shadowed sibling copies get their own emitted fns registered as
# singleton super targets (the own_impls distinction, singleton side).
# A chain also crosses from extends into a parent's `def self.x`.
# All three outputs verbatim from ruby 4.0.6.

module A
  def who; "A -> top"; end
end
module B
  def who; "B -> " + super; end
end
module C
  def who; "C -> " + super; end
end
class Host
  extend A
  extend B
  extend C
  def self.who; "Host -> " + super; end
end
puts Host.who
class Host2
  extend A
  extend B
end
puts Host2.who
class Parent
  def self.who; "Parent -> top"; end
end
class Kid < Parent
  extend B
  def self.who; "Kid -> " + super; end
end
puts Kid.who
__END__
Host -> C -> B -> A -> top
B -> A -> top
Kid -> B -> Parent -> top

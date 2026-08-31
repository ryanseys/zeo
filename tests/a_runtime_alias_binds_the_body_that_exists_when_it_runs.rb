# `alias_method` copies the method entry that exists WHEN IT RUNS, so a `def`
# written below it does not reach the alias. The row is concealed until the
# `def`'s own line, which is also what makes the name undefined above it.
class T1; end
T1.class_eval { alias_method :eql?, :== }
class T1
  def ==(other) = true
end
a, b = T1.new, T1.new
p [a == b, a.eql?(b), a.eql?(a)]
p T1.instance_method(:eql?).owner.to_s

# The `send` spelling reaches the same place.
class T2; end
T2.send(:alias_method, :same?, :==)
class T2
  def ==(other) = true
end
p [T2.new == T2.new, T2.new.same?(T2.new)]

# An alias taken BELOW the def copies the def, and a later redefinition does
# not reach it.
class T4
  def label = "one"
end
T4.class_eval { alias_method :tag, :label }
class T4
  def label = "two"
end
p [T4.new.label, T4.new.tag]

# Two defs of the name with the alias between them.
class T5
  def v = 1
end
T5.class_eval { alias_method :w, :v }
class T5
  def v = 2
end
p [T5.new.v, T5.new.w]

# The class-method channel, where the source is an inherited `def self.x`.
class T7base; def self.tag = "base"; end
class T7 < T7base; end
T7.singleton_class.class_eval { alias_method :made, :tag }
class T7
  def self.tag = "own"
end
p [T7.tag, T7.made]

# A COMPUTED source name leaves every def eager -- the documented limit.
class T8
  def base = 1
end
nm = :base
T8.class_eval { alias_method :copy, nm }
p T8.new.copy

# The compile-time `alias` keyword is a different path and is unchanged.
class T9
  def one = 1
  alias two one
  def one = 11
end
p [T9.new.one, T9.new.two]

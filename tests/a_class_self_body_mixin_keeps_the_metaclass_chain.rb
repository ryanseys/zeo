# A mixin written inside a `class << self` body reaches the singleton class,
# and a singleton class's ancestry is ruby's parallel METACLASS chain --
# `#<Class:Object>`, `#<Class:BasicObject>`, `Class`, `Module` -- not the
# ordinary `Object`-rooted one.
#
# `class << self`'s three mixin verbs mean three different things:
#
#   include M   mixes M into the singleton class, so M's instance methods
#               become the enclosing class's CLASS methods, behind its own.
#               zeo lowers it to `extend M`, which is ruby's own meaning.
#   prepend M   the same, AHEAD of the class's own `def self.x`.
#   extend M    mixes M into the SINGLETON's singleton, one level further
#               out, so it is a run-time send rather than an ancestry edit.
#
# A subclass carries its parent's whole singleton LAYER, not just the
# parent's singleton class: `Child < Inc` lists `SA` between `#<Class:Inc>`
# and `#<Class:Object>`.
module SA
  def s = "SA(#{defined?(super) ? super : 'top'})"
end
module SB
  def s = "SB(#{defined?(super) ? super : 'top'})"
end

class Bare; end
p Bare.singleton_class.ancestors.map(&:to_s)

class Inc
  def self.s = "Inc"
  class << self
    include SA
  end
end
p Inc.s
p Inc.singleton_class.include?(SA)
p Inc.singleton_class.ancestors.map(&:to_s)
p Inc.singleton_class.instance_methods(false).sort

class Pre
  def self.s = "Pre"
  class << self
    prepend SA
  end
end
p Pre.s
p Pre.singleton_class.include?(SA)
p Pre.singleton_class.ancestors.map(&:to_s)

class Both
  def self.s = "Both"
  class << self
    include SA
    prepend SB
  end
end
p Both.s
p Both.singleton_class.ancestors.map(&:to_s)

# The call form of the same two mixins builds the same ancestry.
class Called
  def self.s = "Called"
end
Called.singleton_class.include(SA)
Called.singleton_class.prepend(SB)
p Called.singleton_class.ancestors.map(&:to_s)

# A subclass sees its own metaclass chain continue through the PARENT's.
class Child < Inc; end
p Child.s
p Child.singleton_class.ancestors.map(&:to_s)

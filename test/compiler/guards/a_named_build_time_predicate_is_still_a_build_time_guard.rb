# A gem that gives its build-time question a name asks it as an ordinary
# method call -- `if Sass::Util.rbx?` -- and what sits behind the name is a
# guard the compiler already decides. sass wraps its whole StringScanner class
# in one, with a DIFFERENT superclass per branch, so a compiler that cannot
# decide it registers both arms and reports a superclass mismatch for a program
# ruby runs without complaint.

module Engine
  extend self

  # The memoized spelling, the shape sass writes: the cache holds a constant,
  # so every call answers the same whether or not it is the first.
  def rbx?
    return @rbx if defined?(@rbx)
    @rbx = RUBY_ENGINE == "rbx"
  end

  def mri? = RUBY_ENGINE == "ruby"
end

if Engine.rbx?
  class Scanner < String
    def which = :rubinius
  end
else
  class Scanner < Array
    def which = :mri
  end
end

p Scanner.superclass
p Scanner.new.which

# Only the VALUE folds. The predicate is still compiled and still callable --
# what a decided guard drops is one call whose whole effect was to compute a
# constant, and the memo it would have written is unobservable outside itself.
p Engine.rbx?
p Engine.mri?

# `def self.x` reaches the same fold. lutaml-model gates a whole file of
# `Mutex`/`ConditionVariable` shims on one of these.
module Compat
  def self.opal?
    return @opal if defined?(@opal)
    @opal = RUBY_ENGINE == "opal"
  end
end

unless Compat.opal?
  class Native
    def which = :native
  end
end
p Native.new.which

# A predicate whose body zeo cannot decide leaves its guard to run, exactly as
# an undecidable guard written inline would.
module Runtime
  def self.enabled? = ENV.fetch("ZEO_NAMED_PREDICATE_UNSET", "off") == "on"
end

if Runtime.enabled?
  p :on
else
  p :off
end
__END__
Array
:mri
false
true
:native
:off

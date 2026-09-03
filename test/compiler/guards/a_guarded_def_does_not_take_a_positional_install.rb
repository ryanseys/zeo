module Probe
  # A question no compile-time fold can answer, the way bundler asks
  # `Gem::Platform.local.cpu`. Every guard below reads it.
  def self.cpu
    ["arm", "64"].join
  end
end

# Two units, the GUARDED reopen walked first -- the ordering that let a
# guarded body take the unguarded one's document position.
require_relative "a_guarded_def_does_not_take_a_positional_install/base"
unit = Probe::Unit.new
p unit.extensions_dir
p Probe::Unit.channel
p Probe::Unit.const_defined?(:ONLY_IF_UNIVERSAL)
unit.tag = "tagged"
p unit.tag

# The same pair in one file: guarded first, unguarded second.
module Probe
  class GuardedFirst
    [:tag].each { |n| attr_accessor(n) }
    if Probe.cpu == "universal"
      ONLY = "universal"
      def which = ONLY
      def self.which = ONLY
    end
  end

  class GuardedFirst
    def which = "real"
    def self.which = "real class method"
  end
end
p Probe::GuardedFirst.new.which
p Probe::GuardedFirst.which

# ...and the other order.
module Probe
  class GuardedLast
    [:tag].each { |n| attr_accessor(n) }
    def which = "real"
  end

  class GuardedLast
    if Probe.cpu == "universal"
      def which = "universal"
    end
  end
end
p Probe::GuardedLast.new.which

# A guard that is TRUE still installs, or the fix would just be "never
# install a guarded body".
module Probe
  class GuardTaken
    [:tag].each { |n| attr_accessor(n) }
    def which = "real"
    def self.which = "real class method"
    if Probe.cpu == "arm64"
      def which = "taken"
      def self.which = "taken class method"
    end
  end
end
p Probe::GuardTaken.new.which
p Probe::GuardTaken.which

# A name only a false branch writes is absent, and so is its constant.
module Probe
  class OnlyGuarded
    [:tag].each { |n| attr_accessor(n) }
    if Probe.cpu == "universal"
      NEVER = 1
      def missing = NEVER
    end
  end
end
p Probe::OnlyGuarded.instance_methods(false).include?(:missing)
p Probe::OnlyGuarded.const_defined?(:NEVER)
begin
  Probe::OnlyGuarded.new.missing
rescue NoMethodError => e
  p e.class.to_s
end

# Three bodies for one name, two of them guarded: the counts differ by two.
module Probe
  class ThreeBodies
    [:tag].each { |n| attr_accessor(n) }
    if Probe.cpu == "universal"
      def which = "first guard"
    end
    if Probe.cpu == "sparc"
      def which = "second guard"
    end
    def which = "real"
  end
end
p Probe::ThreeBodies.new.which

# Bundler's actual nesting: a guard inside a guard, with a local assigned
# between them.
module Probe
  class Nested
    [:tag].each { |n| attr_accessor(n) }
    if Probe.cpu
      cpu = Probe.cpu
      if cpu == "universal"
        DEEP = "deep"
        def which = DEEP
      end
    end
    def which = "real"
  end
end
p Probe::Nested.new.which

# The pass's own job, unchanged: a block-installed accessor loses to the
# `def` written below it, and so does a macro a class method expands.
module Probe
  class StillPositional
    [:y].each { |a| attr_writer(a) }
    def y=(v)
      @y = [v, v]
    end
    attr_reader :y
  end

  class MacroPack
    def self.make(n) = attr_writer(n)
    make :z
    def z=(v)
      @z = [v, v]
    end
    attr_reader :z
  end
end
s = Probe::StillPositional.new
s.y = 1
p s.y
m = Probe::MacroPack.new
m.z = 2
p m.z
__END__
"real"
"real class method"
false
"tagged"
"real"
"real class method"
"real"
"taken"
"taken class method"
false
false
"NoMethodError"
"real"
"real"
[1, 1]
[2, 2]

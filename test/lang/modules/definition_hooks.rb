# `Module#included`/`#extended`/`#prepended` and `Class#inherited` -- the four
# hooks a mixin or a subclass fires as it is written. The extend-on-include
# idiom below (a module giving its host class-side methods) is what
# ActiveSupport::Concern, Syslog::Constants, and most every Ruby DSL are built
# out of, so a module whose `included` never ran left its host without them.
#
# Each hook runs AFTER the edit it reports, at the mixin's own document
# position, and `inherited` runs once per class -- at creation, not per reopen.

module Hooked
  def hi = "instance"

  def self.included(base)
    puts "included #{base}"
    base.extend(self)
  end

  def self.extended(base)
    puts "extended #{base}"
  end

  def self.prepended(base)
    puts "prepended #{base}"
  end
end

class Host
  puts "body starts"
  include Hooked
  puts "body ends"
end

p Host.new.hi
p Host.hi

class Extender
  extend Hooked
end
p Extender.hi

class Prepender
  prepend Hooked
end
p Prepender.new.hi

# A module with no hook stays silent, and the mixin still works.
module Quiet
  def quiet = "quiet"
end
class Silent
  include Quiet
end
p Silent.new.quiet

# --- `included` sees the ancestry the edit already made ---------------------
module Reflective
  def self.included(base)
    p base.ancestors.include?(Reflective)
    p base.instance_methods(false).sort
  end

  def helper = 1
end
class Reflected
  def own = 2
  include Reflective
end

# --- `inherited` fires once, at creation ------------------------------------
class Tracked
  def self.inherited(sub)
    puts "inherited #{sub}"
    super
  end
end
class ChildA < Tracked; end
class ChildB < Tracked
  puts "child b body"
end
class ChildB
  puts "child b reopened"
end
class GrandChild < ChildA; end

# --- the runtime shapes fire too --------------------------------------------
# An anonymous class and a bare object both print an address, so these report
# only WHICH hook ran.
module Counting
  def self.seen = (@seen ||= [])
  def self.included(base) = seen << :included
  def self.extended(base) = seen << :extended
  def self.prepended(base) = seen << :prepended
  def counted = "counted"
end

runtime = Class.new do
  include Counting
end
p runtime.new.counted

obj = Object.new
obj.extend(Counting)
p obj.counted
p Counting.seen
__END__
body starts
included Host
extended Host
body ends
"instance"
"instance"
extended Extender
"instance"
prepended Prepender
"instance"
"quiet"
true
[:own]
inherited ChildA
inherited ChildB
child b body
child b reopened
inherited GrandChild
"counted"
"counted"
[:included, :extended]

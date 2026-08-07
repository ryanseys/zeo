# `class X < Module` -- a module FACTORY, whose instances are real modules.
#
# This is NOT the value-subclass payload shape and must not become it: a
# wrapper holding a module is not a module, so it would fail `include`,
# `Module#===`, constant lookup and `ancestors`. Instead `X.new` mints a real
# runtime module id and tags it as belonging to X, so only the question "what
# is your class?" changes answer.
#
# 22 of the gem corpus's `Module` rows come from ONE file --
# `activesupport/lib/active_support/deprecation/proxy_wrappers.rb` -- which
# gates actioncable, actionmailer, actionview, activejob, activemodel,
# activerecord, activestorage and the rest of the Rails stack.

class LazilyDefineAttributes < Module
  def initialize(attributes)
    @attributes = attributes
  end

  def included(base)
    base.instance_variable_set(:@lazy, @attributes)
  end

  def attributes
    @attributes
  end
end

m = LazilyDefineAttributes.new([:a, :b])

# Its class is the subclass, and it is a module all the same.
p [m.class, m.is_a?(Module), m.is_a?(LazilyDefineAttributes), m.instance_of?(LazilyDefineAttributes)]
p Module === m

# A user instance method runs against it, and ivars persist across calls --
# `initialize` wrote `@attributes`, `attributes` reads it back.
p m.attributes

# `include`ing one runs the user `included` hook, because the value really is
# a module: `Module#include` reaches it unchanged.
class Host
  include LazilyDefineAttributes.new([:x])
end

p Host.instance_variable_get(:@lazy)
p Host.ancestors.include?(Module)

# Reflection on the factory itself.
p LazilyDefineAttributes.superclass
p LazilyDefineAttributes.instance_of?(Class)

# Ivars set from OUTSIDE reach the same store the method bodies use.
m.instance_variable_set(:@extra, 9)
p [m.instance_variable_get(:@extra), m.instance_variables.sort]

# The Rails shape itself: a `self.new` override that can answer a NON-module,
# and a subclass that inherits `initialize`.
class DeprecatedConstantProxy < Module
  def self.new(*args, **options, &block)
    object = args.first
    return object unless object
    super
  end

  def initialize(old_const, new_const)
    @old_const = old_const
    @new_const = new_const
  end

  def inspect = "DeprecatedConstantProxy(#{@old_const} -> #{@new_const})"

  def target = @new_const
end

# `new` returning something that is not an instance of the class at all.
p DeprecatedConstantProxy.new(nil)

pr = DeprecatedConstantProxy.new("OLD", "NEW")
p [pr.class, pr.inspect, pr.target, pr.is_a?(Module)]

class Sub < DeprecatedConstantProxy
end

s = Sub.new("A", "B")
p [s.class, s.target, s.is_a?(DeprecatedConstantProxy), s.is_a?(Module)]

# A factory that defines no `initialize` at all takes no arguments -- `Module`'s
# own must not run in its place.
class Bare < Module
end

b = Bare.new
p [b.class, b.is_a?(Module)]

# `ObjectSpace::WeakMap` is the third native shape, and the cleanest: its
# constructor already builds `WeakMap::new(class)` from the receiver, so a
# subclass instance IS the native type -- no payload wrapper, no re-tagging.
# activesupport's `DescendantsTracker::WeakSet` is the case, and it gates
# eleven Rails gems on its own.
class WeakSet < ObjectSpace::WeakMap
  alias_method :to_a, :keys

  def <<(object)
    self[object] = true
    self
  end
end

ws = WeakSet.new
p [ws.class, ws.is_a?(ObjectSpace::WeakMap), WeakSet.superclass]

one = "one"
two = "two"
ws << one
ws << two
p [ws.to_a.map(&:itself).sort, ws.size, ws[one]]

# `Date`/`DateTime` are the fourth native shape, and the mirror image of a
# payload root: the rows already allocate through the RECEIVER
# (`RDate::new(jdn, class_of(recv))`, exactly as the equivalent CRuby function
# threads `klass`), and an `RDate` carries its class id directly -- so there is
# nothing to wrap and nothing to re-tag. tzinfo's `DateTimeWithOffset` is the
# case, and activesupport reaches the ledger through it.
require "date"

class DateTimeWithOffset < DateTime
  attr_accessor :timezone_offset

  def set_timezone_offset(o)
    @timezone_offset = o
    self
  end
end

dt = DateTimeWithOffset.jd(2460000)
p [dt.class, dt.is_a?(DateTime), dt.is_a?(Date), dt.year, dt.month, dt.day]

# `RDate` carries no ivar storage of its own, so `@timezone_offset` lands in
# the identity-keyed side table and reads back from it.
p dt.set_timezone_offset("+09:00").timezone_offset

# Every inherited constructor tags with the receiver, `new` included --
# `Date.new` IS `Date.civil`, a class-method constructor.
class MyDate < Date
end

p [MyDate.civil(2001, 2, 3).class, MyDate.parse("2001-02-03").class, MyDate.today.class]
p [DateTimeWithOffset.new(2024, 5, 6).class, DateTimeWithOffset.superclass]
p MyDate.civil(2001, 2, 3).to_s

# The roots themselves are untouched.
p [Date.today.class, DateTime.jd(2460000).class, Date.civil(2001, 2, 3).to_s]

# `Proc` is the fifth native shape, and deliberately NOT a payload wrapper:
# every call-site fast path, `&blk` conversion and `to_proc` matches on
# `RubyValue::Proc`, so boxing one inside an object would break all of them.
# The class rides in the proc itself instead. declarative's
# `Declarative::Variables::Proc` is the case -- an empty subclass used purely
# as a tag (`v.is_a?(Variables::Proc)`) on something that is still called.
class Tagged < ::Proc
end

t = Tagged.new { |x| x * 2 }
p [t.class, t.is_a?(Proc), t.is_a?(Tagged), Tagged.superclass]
p [t.call(21), t.(21), t[21], t.arity, t.lambda?]

# It is a real Proc everywhere a Proc is expected -- including as a block, which
# is where a wrapper object would have failed.
p [1, 2, 3].map(&t)

def takes_block(&b)
  b.class
end

p takes_block(&t)

# ...and the base class is untouched.
p [Proc.new { 1 }.class, t.curry.class]

# A `def self.x` added to a REOPENED builtin -- or written in a `class << Time`
# body -- is a class method like any other, so it has to answer a DYNAMIC send
# too, not only a call zeo resolved statically. It used to answer just the
# static one: `Time.tag` worked while `Time.__send__(:tag)` and the `self.tag`
# inside a sibling class method both raised NoMethodError.
class Time
  def self.tag = "time-tag"
  def self.via_self = self.tag
  def self.via_send = __send__(:tag)
  def self.via_const = Time.tag
  def self.bare = tag
end

class String
  class << String
    def label = "string-label"
    def relay = self.label
  end
end

p Time.tag
p Time.via_self
p Time.via_send
p Time.via_const
p Time.bare
p String.label
p String.relay

# Reached through a variable, which is the case codegen cannot resolve at all.
[Time, String].each { |k| p k.respond_to?(k == Time ? :tag : :label) }
holder = Time
p holder.tag
p holder.public_send(:tag)

# Reflection agrees: the method sits on the singleton class.
# (`Time.method(:tag).owner` is left out -- zeo answers the class where ruby
# answers its singleton class, for a USER class equally, so it is a separate
# divergence rather than anything this file is about.)
p Time.singleton_methods.include?(:tag)
p String.singleton_methods.include?(:label)
p Time.singleton_class.instance_methods(false).include?(:tag)
p Time.method(:tag).arity
p Time.respond_to?(:tag)
p Time.respond_to?(:no_such_tag)

# An argument-taking one, and a block-taking one, both through the dynamic path.
class Array
  def self.doubled(n) = n * 2
  def self.wrap = yield 3
end
p Array.__send__(:doubled, 21)
p Array.__send__(:wrap) { |v| v + 1 }
p Array.method(:doubled).arity

# A module reopen takes the same path.
module Comparable
  def self.mark = :marked
end
p Comparable.__send__(:mark)
p Comparable.singleton_methods.include?(:mark)

# The builtin's OWN class methods still answer, and a user row beats nothing it
# should not: `Time.now` is untestable but `Integer.sqrt` is exact.
p Integer.__send__(:sqrt, 17)
__END__
"time-tag"
"time-tag"
"time-tag"
"time-tag"
"time-tag"
"string-label"
"string-label"
true
true
"time-tag"
"time-tag"
true
true
true
0
true
false
42
4
1
:marked
true
4

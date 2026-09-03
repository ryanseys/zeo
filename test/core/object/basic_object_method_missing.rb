# A `BasicObject` subclass's `method_missing` is never reached: zeo raises
# `NoMethodError` from the send instead of calling it.
#
# `BasicObject` is the blank slate you inherit from precisely to catch
# everything -- it has seven methods, so almost every call is a miss, and
# `method_missing` is the whole mechanism. Every proxy/delegator built this way
# (`Delegator`, `BasicObject`-rooted builders, the classic `NullObject`) stops
# working.
#
# The same class rooted at `Object` DOES reach its `method_missing` (the second
# pair below), so the fallback exists and the BasicObject-rooted case misses
# it. `dispatch::send_in_reason` looks the hook up through the receiver's
# registry chain; a BasicObject subclass's chain is the one place that lookup
# does not find a user row.

class Blank < BasicObject
  def method_missing(name, *args) = [:blank_mm, name, args]
  def respond_to_missing?(name, priv = false) = true
end

b = Blank.new
p b.anything
p b.with(1, 2)
p b.respond_to?(:whatever)

# Rooted at Object, the same shape already works.
class Normal
  def method_missing(name, *args) = [:normal_mm, name, args]
  def respond_to_missing?(name, priv = false) = true
end
p Normal.new.anything
p Normal.new.respond_to?(:whatever)

p ::BasicObject.instance_methods(false).sort
__END__
[:blank_mm, :anything, []]
[:blank_mm, :with, [1, 2]]
[:blank_mm, :respond_to?, [:whatever]]
[:normal_mm, :anything, []]
true
[:!, :!=, :==, :__id__, :__send__, :equal?, :instance_eval, :instance_exec]

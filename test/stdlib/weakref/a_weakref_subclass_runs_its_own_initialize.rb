# `class TaggedRef < WeakRef` used to be registered with the ROOT's own
# constructor, which seated the referent straight from the args -- the
# subclass's `initialize` never ran and `@tagged` read back nil.
#
# A `WeakRef` is now allocated UNSEATED and `initialize` seats it, so a
# subclass's body runs and its `super(obj)` reaches `WeakRef#initialize`, the
# same split CRuby's pure-Ruby weakref.rb has. That split also made the rest
# of the surface expressible, and the oracle disagreed with zeo on all of it:
#
#   - `__setobj__` is a NO-OP answering nil. The referent lives in a map keyed
#     on the WeakRef object, which `__setobj__` never touches; the method
#     exists only because `Delegator` demands it.
#   - `weakref_alive?` is `@@__map.key?(self) or defined?(@delegate_sd_obj)`,
#     so it answers `true`, `defined?`'s STRING, or nil -- never `false`.
#   - `#dup` LOSES the referent (the copy is a different map key). Only a
#     `true`/`false`/`nil` referent survives, because that one is stashed in
#     an ivar rather than the map.
require "weakref"

class TaggedRef < WeakRef
  def initialize(obj)
    super(obj)
    @tagged = true
  end

  def tagged? = @tagged
end

s = +"referent"
r = TaggedRef.new(s)
p r.class
p r.tagged?
p r.upcase
p r.weakref_alive?

# A subclass with no body of its own still works.
class PlainRef < WeakRef; end
p PlainRef.new(s).upcase

w = WeakRef.new(s)
p w.__setobj__(+"ignored")
p w.__getobj__

# The three referents that live in an ivar instead of the map.
p WeakRef.new(true).weakref_alive?
p WeakRef.new(true).__getobj__
p WeakRef.new(false).__getobj__
p WeakRef.new(nil).__getobj__
# ... and two immediates that do go in the map.
p WeakRef.new(7).__getobj__
p WeakRef.new(:sym).weakref_alive?

# A copy keeps only the ivar-held kind.
p w.dup.weakref_alive?
begin
  w.dup.__getobj__
rescue WeakRef::RefError => e
  p [e.class.to_s, e.message]
end
d = WeakRef.new(true).dup
p [d.weakref_alive?, d.__getobj__]

begin
  WeakRef.new
rescue ArgumentError => e
  p e.message
end
begin
  WeakRef.new(s, 2)
rescue ArgumentError => e
  p e.message
end
__END__
TaggedRef
true
"REFERENT"
true
"REFERENT"
nil
"referent"
"instance-variable"
true
false
nil
7
true
nil
["WeakRef::RefError", "Invalid Reference - probably recycled"]
["instance-variable", true]
"wrong number of arguments (given 0, expected 1)"
"wrong number of arguments (given 2, expected 1)"

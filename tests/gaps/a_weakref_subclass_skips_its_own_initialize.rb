# The WeakRef flavor of the receiver-built-constructor gap (the WeakMap half
# is fixed and promoted): a `class TaggedRef < WeakRef` is registered with the
# root's own constructor, which seats the referent straight from the args --
# the subclass's own `initialize` never runs, so `@tagged` reads back nil.
#
# WeakMap's fix (build native, then the standard `run_initialize` tail) does
# not transplant: WeakRef's native object cannot exist WITHOUT a referent, so
# running the user body first needs an empty native + a `super(obj)` re-seat
# channel -- the same empty-payload + value_super shape the payload roots use.
require "weakref"

class TaggedRef < WeakRef
  def initialize(obj)
    super(obj)
    @tagged = true
  end

  def tagged? = @tagged
end

s = "referent"
r = TaggedRef.new(s)
p r.class
p r.tagged?
p r.upcase

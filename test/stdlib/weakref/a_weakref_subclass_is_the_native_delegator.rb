# `WeakRef`'s constructor takes the receiver class, so a subclass IS the
# native delegator -- no payload wrapper, no re-tagging. What a subclass
# needs beyond that is its own copy of the two delegation rows: the send-miss
# fallback looks `method_missing` up on the receiver's own class, so without
# them every forwarded name raises instead. hexapdf's `NullableWeakRef` is
# the shape, and it reaches the corpus through thirteen gems.
require "weakref"

class NullableWeakRef < WeakRef
  def __getobj__
    super
  rescue StandardError
    nil
  end

  def shout = "#{__getobj__.upcase}!"
end

target = "held"
ref = NullableWeakRef.new(target)

p ref.class
p NullableWeakRef.superclass
p ref.is_a?(WeakRef)
p ref.weakref_alive?
p ref.__getobj__
p ref.shout
p ref.upcase
p ref.length
p ref.respond_to?(:upcase)
p ref.respond_to?(:no_such_method_at_all)
__END__
NullableWeakRef
WeakRef
true
true
"held"
"HELD!"
"HELD"
4
true
false

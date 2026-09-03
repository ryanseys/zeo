# An exception subclass whose `initialize` takes KEYWORD arguments (D3): the
# runtime construction path (`construct_by_class_id`) must carry keywords as
# the trailing-Hash the trampoline expects, or `code:` would default. Two
# sibling subclasses -- neither's `message` param pinned to a String -- also
# exercise the poly `super(message)` coercion into the native message slot.

class AError < StandardError
  attr_reader :code
  def initialize(message, code: 3)
    super(message)
    @code = code
  end
end
class BError < StandardError
  attr_reader :level
  def initialize(message, level = 7)
    super(message)
    @level = level
  end
end
a = AError.new("boom", code: 9)
puts a.code
puts a.message
b = BError.new("bad", 5)
puts b.level
puts b.message
puts AError.new("d").code
__END__
9
boom
5
bad
3

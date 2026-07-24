# #684: a concretely-typed receiver's attr_reader must not widen to
# poly just because an unrelated class shares the same attr name.
class Bag
  attr_accessor :payload
  def initialize; @payload = {"k" => "v"}; end
end

class Wire
  attr_accessor :payload
  def initialize; @payload = ""; end
end

def first_byte_of(s)
  s.bytes[0]
end

def handle_wire(w)
  first_byte_of(w.payload)
end

_b = Bag.new
_w = Wire.new
_w.payload = "hello"
puts handle_wire(_w)

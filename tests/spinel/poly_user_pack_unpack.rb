# The poly-receiver arms for `pack` and `unpack1` did not ask whether a user
# class owns the name, the way their neighbours do. `pack` answered "" and
# `unpack1` read the receiver through a char * whatever it held -- both
# silently, neither entering the method (#4071's shape, found by sweeping the
# poly arms for the guard).
class Frame
  def initialize
    @packed = 0
  end

  attr_reader :packed

  def pack(fmt)
    @packed += 1
    "frame:" + fmt
  end
end

class Wire
  def unpack1(fmt)
    "wire:" + fmt
  end
end

def packed(v) = v.pack("C*")
def first_of(v) = v.unpack1("C")

f = Frame.new
p packed(f)
p f.packed
p packed([65, 66])
p packed([1.5])

p first_of(Wire.new)
p first_of("AB")

# and with no user class in sight the plain lowering is unchanged
p [65, 66].pack("C*")
p "AB".unpack1("C")
p "AB".unpack1("a")

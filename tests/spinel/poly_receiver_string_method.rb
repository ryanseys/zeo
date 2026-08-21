# A String method called on a POLY receiver. The receiver is only known at run
# time (a container read, a yield), and the arms below coerced it with
# sp_poly_to_s -- so a receiver that was not a string got the String method
# applied to its #to_s RENDERING and answered in silence:
#
#   [1, Bare.new][1].upcase   =>  "#<BARE:0X0000560F...>"
#
# CRuby has no such method on the object and raises NoMethodError. Implicit
# conversion does NOT rescue it either: #to_str converts an ARGUMENT into a
# String slot and never a receiver, so a class defining #to_str still raises
# here -- which is why this is not the conversion protocol's business.
class Bare
end

class WithToStr
  def to_str
    "CONV"
  end

  def to_s
    "SHOW"
  end
end

def try(label)
  yield
rescue NoMethodError => e
  p [label, e.class, e.message]
end

o = [1, Bare.new][1]
try("upcase")  { p o.upcase }
try("strip")   { p o.strip }
try("reverse") { p o.reverse }
try("chars")   { p o.chars }

# #to_str does not make the receiver a String
w = [1, WithToStr.new][1]
try("to_str upcase") { p w.upcase }

# every receiver kind that IS a string still answers, and the ones CRuby
# refuses are refused: a Symbol has its own case conversion but no #strip,
# and a shared-mutable string is a builtin box rather than a bare tag.
p [1, "Ab c"][1].upcase
p [1, String.new("Ab c")][1].upcase
p [1, String.new("Ab c")][1].chars
p [1, :ab][1].upcase
try("sym strip") { p [1, :ab][1].strip }
try("int upcase") { p [1, 5][1].upcase }
try("nil strip")  { p [1, nil][1].strip }

# a user class that owns the name keeps its own arm
class Owns
  def upcase
    "MINE"
  end
end
p [1, Owns.new][1].upcase

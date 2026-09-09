# `case obj when ClassConst` on a receiver whose type is not known until run
# time. Module#=== is `obj.is_a?(mod)`, so each arm walks the real
# ancestors: Integer, String, Float and Symbol arms, several in one case,
# and the else clause.

def describe(v)
  case v
  when Integer
    "int"
  when String
    "string"
  when Float
    "float"
  when Symbol
    "symbol"
  else
    "other"
  end
end

puts describe(42)
puts describe("hello")
puts describe(3.14)
puts describe(:sym)
puts describe(nil)
__END__
int
string
float
symbol
other

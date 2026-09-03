# A range pattern in `case/in` is `Range#===` -- a `#cover?` test that works
# for any comparable scrutinee (a Float, or a value of unknown static type),
# an exclusive range, and a beginless/endless range, not just integers.
def classify(x)
  case x
  in 0.0...0.5 then "low"
  in 0.5..1.0  then "high"
  in ..0.0     then "negative"
  else              "other"
  end
end
puts classify(0.2)
puts classify(0.9)
puts classify(-1.0)
puts classify(5.0)

# A poly element (its type isn't statically known) matches an integer range
# by value; a non-integer simply doesn't match.
[3, "x", 7].each do |v|
  case v
  in 0..5 then puts "#{v.inspect}: small int"
  else         puts "#{v.inspect}: other"
  end
end
__END__
low
high
negative
other
3: small int
"x": other
7: other

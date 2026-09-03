# A range pattern is Range#=== (rb_case_eq/range_covers), so a Float or
# Poly scrutinee, an exclusive bound, and a beginless/endless range all
# work -- an as_int_unchecked path would panic on non-Int scrutinees.

def classify(x)
  case x
  in 0.0...0.5 then "low"
  in 0.5..1.0 then "high"
  in ..0.0 then "neg"
  else "other"
  end
end
puts classify(0.2)
puts classify(0.9)
puts classify(-1.0)
puts classify(5.0)
arr = [3, "x"]
case arr[0]
in 0..3 then puts "small"
else puts "big"
end
case arr[1]
in 0..3 then puts "small"
else puts "not-int"
end
__END__
low
high
neg
other
small
not-int

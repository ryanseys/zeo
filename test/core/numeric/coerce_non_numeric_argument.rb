# `Integer#coerce` / `Float#coerce` raise `Float()`'s own errors.
#
# Ruby's `num_coerce` is literally `[Float(y), Float(x)]`, so a failure
# names the VALUE and `Float` (`can't convert nil into Float`) rather than
# reporting a coercion failure naming the class and the receiver's type.
# An INTEGER pair stays integral, which is `rb_int_coerce`'s own rule.
#
# The fix closed one more divergence with it: `5.coerce(Rational(1,2))`
# raised where ruby answers `[0.5, 5.0]`.
#
# The original header follows. It describes the PREDECESSOR project's
# version of this test and its own fix.
#
# Integer#coerce / Float#coerce with a non-numeric argument. CRuby answers
# `[Float(other), Float(self)]`, so the errors are Float()'s: a TypeError for
# nil / true / an Array / a Symbol, and an ArgumentError for an unparseable
# String. spinel put the argument straight into the Integer pair's slot, so a
# String stopped the C BUILD and a nil answered a coerced 0 (#4011).

[nil, true, false, "x", [1], { a: 1 }, :s].each do |v|
  begin
    p 5.coerce(v)
  rescue => e
    puts "#{v.inspect} => #{e.class}: #{e.message}"
  end
  begin
    p 1.5.coerce(v)
  rescue => e
    puts "1.5 #{v.inspect} => #{e.class}: #{e.message}"
  end
end

# the same, written as literals at the call
begin; p 5.coerce("x"); rescue => e; puts "#{e.class}: #{e.message}"; end
begin; p 5.coerce([1]); rescue => e; puts "#{e.class}: #{e.message}"; end
begin; p 5.coerce(nil); rescue => e; puts "#{e.class}: #{e.message}"; end
begin; p 1.5.coerce(nil); rescue => e; puts "#{e.class}: #{e.message}"; end

# a parseable String is still a number
p 5.coerce("2.5")

# and the numeric pairs are unchanged
p 5.coerce(2)
p 5.coerce(2.5)
p 5.coerce(2**70)
p 1.5.coerce(3)
v = [1, 2][1]
p 5.coerce(v)
__END__
nil => TypeError: can't convert nil into Float
1.5 nil => TypeError: can't convert nil into Float
true => TypeError: can't convert true into Float
1.5 true => TypeError: can't convert true into Float
false => TypeError: can't convert false into Float
1.5 false => TypeError: can't convert false into Float
"x" => ArgumentError: invalid value for Float(): "x"
1.5 "x" => ArgumentError: invalid value for Float(): "x"
[1] => TypeError: can't convert Array into Float
1.5 [1] => TypeError: can't convert Array into Float
{a: 1} => TypeError: can't convert Hash into Float
1.5 {a: 1} => TypeError: can't convert Hash into Float
:s => TypeError: can't convert Symbol into Float
1.5 :s => TypeError: can't convert Symbol into Float
ArgumentError: invalid value for Float(): "x"
TypeError: can't convert Array into Float
TypeError: can't convert nil into Float
TypeError: can't convert nil into Float
[2.5, 5.0]
[2, 5]
[2.5, 5.0]
[1180591620717411303424, 5]
[3.0, 1.5]
[2, 5]

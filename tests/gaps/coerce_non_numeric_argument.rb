# GAP -- imported from the spinel corpus at fa06b601.
#
# `Integer#coerce` / `Float#coerce` with a non-numeric argument raise the
# WRONG TypeError message, and from the wrong place.
#
#   1.coerce(nil)   CRuby "can't convert nil into Float"
#                   zeo   "can't coerce NilClass into Integer"
#
# CRuby's `coerce` answers `[Float(other), Float(self)]`, so the error is
# `Float()`'s and it names the VALUE (`nil`) and the target (`Float`). zeo
# reports its own coercion failure instead, naming the CLASS and the
# receiver's type. Both are TypeErrors, so a rescue still fires; a message
# match does not.
#
# The original header follows. It describes the PREDECESSOR project's
# version of this test and its own fix, not zeo's divergence above.
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

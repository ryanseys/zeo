# `deconstruct` must answer an Array and `deconstruct_keys` a Hash. A
# wrong return RAISES -- it does not fall through to the next `in` clause.
#
# The value used to be waved through with `matched = 1` and only failed
# one frame later, in an accessor that panics; that entry is `extern "C"`
# and cannot unwind, so the panic ended the process instead of raising.
# Checking where the value ARRIVES is the fix, and the rule behind it: an
# accessor that panics may not be called from a non-unwinding boundary.

bad_array = Class.new { def deconstruct = :not_array }
begin
  case bad_array.new
  in [x]
    p x
  end
rescue TypeError => e
  puts e.message
end

bad_hash = Class.new { def deconstruct_keys(_keys) = :not_hash }
begin
  case bad_hash.new
  in { a: }
    p a
  end
rescue TypeError => e
  puts e.message
end

# It raises rather than trying the next clause -- an `else` does not catch
# it either.
begin
  case bad_array.new
  in [x]
    p x
  in Object
    puts "fell through"
  end
rescue TypeError => e
  puts e.message
end

# The working protocol is untouched, in both halves.
class Pair
  def deconstruct = [1, 2]
  def deconstruct_keys(_keys) = { a: 1, b: 2 }
end

case Pair.new
in [x, y]
  p [x, y]
end
case Pair.new
in { a:, b: }
  p [a, b]
end

# A raising `deconstruct` propagates its own exception, not a TypeError.
raiser = Class.new { def deconstruct = raise(ArgumentError, "from deconstruct") }
begin
  case raiser.new
  in [_x]
    nil
  end
rescue ArgumentError => e
  puts e.message
end

# An Array and a Hash still match directly, with no protocol call at all.
case [1, 2]
in [p1, p2]
  p [p1, p2]
end
case({ a: 1 })
in { a: }
  p a
end
__END__
deconstruct must return Array
deconstruct_keys must return Hash
deconstruct must return Array
[1, 2]
[1, 2]
from deconstruct
[1, 2]
1

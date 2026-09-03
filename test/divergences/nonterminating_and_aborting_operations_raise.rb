# Guards for operations that would otherwise hang or kill the process.
# A zero step never advances, so CRuby rejects it up front
# (numeric.c:2888) -- via a Ruby-level `==`, so `0.0` trips it too.
# Materializing an endless range would grow a vector until the process
# died (range.c:1023). `String#*` past a sane cap would hand the
# allocator an impossible request, which ABORTS rather than raising;
# zeo caps it and raises a catchable ArgumentError, where CRuby --
# whose guard covers only the length multiplication -- reaches the
# allocator and raises NoMemoryError. That last one is a deliberate,
# documented divergence toward a rescuable failure.

def err
  yield
rescue => e
  "#{e.class}: #{e.message}"
end
puts err { 1.step(10, 0) { } }
puts err { 1.step(10, 0.0) { } }
puts err { Rational(1, 2).step(Rational(5, 2), 0) { } }
puts err { (1..).to_a }
puts err { (1..).entries }
p (1..4).to_a
puts err { "x" * -1 }
puts err { "x" * (1 << 60) }
p "ab" * 3
__END__
ArgumentError: step can't be 0
ArgumentError: step can't be 0
ArgumentError: step can't be 0
RangeError: cannot convert endless range to an array
RangeError: cannot convert endless range to an array
[1, 2, 3, 4]
ArgumentError: negative argument
ArgumentError: string size too big
"ababab"

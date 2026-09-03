# Range, Integer iteration, and Float() long-tail behaviors.

# A Symbol range iterates by name succession, yielding Symbols.
p (:a..:e).to_a
p (:a...:d).to_a
p (:aa..:ac).to_a

# A Float range has O(1) min/max endpoints (it can't be iterated), a
# drift-free step (1.0 lands exactly), and a float bsearch that converges on a
# representable boundary. Iterating one (each/to_a) raises, naming the element
# type; an exclusive float max has no maximum element.
r = (1.0..3.0)
p r.min
p r.max
p r.step(0.5).to_a
p (0.0..1.0).step(0.1).to_a
p (0.0..10.0).bsearch { |x| x >= 3.5 }
p(begin; r.to_a; rescue => e; "#{e.class}: #{e.message}"; end)
p(begin; (1.0...3.0).max; rescue => e; "#{e.class}: #{e.message}"; end)

# Range#step with a negative step walks a descending range.
p (10..2).step(-2).to_a
p (10...1).step(-1).to_a

# Integer#downto / #upto iterate as BigInt when the values exceed i64 (the span
# is small), and the enumerator's size is exact. A Float limit yields integers.
big = 2 ** 100
p big.downto(big - 3).to_a
p big.upto(big + 2).to_a
p big.downto(big - 2).size
p 5.downto(2.0).to_a

# Float() parses C99 hex-float strings (the binary exponent is optional).
p Float("0x1p4")
p Float("0x1.8p1")
p Float("0xa")
p Float("-0x1p4")
__END__
[:a, :b, :c, :d, :e]
[:a, :b, :c]
[:aa, :ab, :ac]
1.0
3.0
[1.0, 1.5, 2.0, 2.5, 3.0]
[0.0, 0.1, 0.2, 0.30000000000000004, 0.4, 0.5, 0.6000000000000001, 0.7000000000000001, 0.8, 0.9, 1.0]
3.5
"TypeError: can't iterate from Float"
"TypeError: cannot exclude non Integer end value"
[10, 8, 6, 4, 2]
[10, 9, 8, 7, 6, 5, 4, 3, 2]
[1267650600228229401496703205376, 1267650600228229401496703205375, 1267650600228229401496703205374, 1267650600228229401496703205373]
[1267650600228229401496703205376, 1267650600228229401496703205377, 1267650600228229401496703205378]
3
[5, 4, 3, 2]
16.0
3.0
10.0
-16.0

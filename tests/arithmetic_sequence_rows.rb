# `Enumerator::ArithmeticSequence` -- what a blockless `Range#step`,
# `Range#%` or `Numeric#step` over a numeric receiver answers, its thirteen
# own methods, and the strided `Array#[]` that reads one.

puts "class: #{(1..10).step(2).class}"
puts "superclass: #{Enumerator::ArithmeticSequence.superclass}"
puts "own methods: #{Enumerator::ArithmeticSequence.instance_methods(false).sort.inspect}"
puts "ancestors head: #{Enumerator::ArithmeticSequence.ancestors.first(4).inspect}"
puts "range %: #{((1..10) % 3).class}"
puts "numeric step: #{1.step(10, 3).class}"
puts "string range: #{("a".."e").step(2).class}"

# The quadruple, which every other row is computed from.
s = (1..10).step(2)
puts "parts: #{[s.begin, s.end, s.step, s.exclude_end?].inspect}"
puts "exclusive parts: #{[(1...10).step(3).end, (1...10).step(3).exclude_end?].inspect}"
puts "endless end: #{(1..).step(2).end.inspect}"
puts "numeric exclude_end?: #{1.step(10, 3).exclude_end?}"

# `#inspect` replays the CALL, so the same sequence prints three ways.
puts "inspect range: #{(1..10).step(2).inspect}"
puts "inspect exclusive: #{(1...10).step(2).inspect}"
puts "inspect endless: #{(1..).step(2).inspect}"
puts "inspect %: #{((1..10) % 2).inspect}"
puts "inspect float: #{(1.0..2.0).step(0.5).inspect}"
puts "inspect numeric: #{1.step(10, 3).inspect}"
puts "inspect bare: #{1.step.inspect}"
puts "inspect kwargs: #{1.step(to: 9, by: 2).inspect}"
puts "inspect rational: #{Rational(1, 2).step(Rational(5, 2), Rational(1, 2)).inspect}"

# `#each` answers self, and a blockless call answers self too rather than
# wrapping the sequence in another enumerator.
seen = []
puts "each self: #{s.each { |x| seen << x }.equal?(s)}"
puts "each yielded: #{seen.inspect}"
puts "each blockless: #{s.each.class}"
puts "to_a: #{s.to_a.inspect}"
puts "map: #{s.map { |x| x * 10 }.inspect}"

# The Float lane computes `begin + i * step` rather than running a sum, so
# the last element lands exactly on the endpoint.
puts "float walk: #{(0.0..1.0).step(0.1).to_a.inspect}"
puts "numeric float walk: #{0.0.step(1.0, 0.1).to_a.inspect}"
puts "mixed walk: #{1.step(2.0, 0.5).to_a.inspect}"
puts "rational walk: #{Rational(1, 2).step(Rational(5, 2), Rational(1, 2)).to_a.inspect}"
puts "descending: #{(10..1).step(-2).to_a.inspect}"
puts "against direction: #{(1..10).step(-2).to_a.inspect}"
puts "endless take: #{(1..).step(2).first(4).inspect}"

# `#size` is computed, never walked -- which is the only way an endless
# sequence answers at all.
puts "size: #{s.size}"
puts "size exclusive: #{(1...10).step(3).size}"
puts "size float: #{(0.0..1.0).step(0.1).size}"
puts "size float exclusive: #{(1...10).step(2.5).size}"
puts "size mixed: #{(1..10.5).step(2).size}"
puts "size empty: #{(1..0).step(2).size}"
puts "size descending: #{(10..1).step(-2).size}"
puts "size endless: #{(1..).step(2).size}"
puts "size rational: #{Rational(1, 2).step(Rational(5, 2), Rational(1, 2)).size}"

# `#first` answers `begin` untouched; `#first(n)` walks, so it inherits the
# walk's Float lane.
puts "first: #{s.first}"
puts "first mixed: #{(1..10.5).step(2).first}"
puts "first n mixed: #{(1..10.5).step(2).first(3).inspect}"
puts "first empty: #{(1..0).step(2).first.inspect}"
puts "first zero: #{s.first(0).inspect}"
puts "first over: #{s.first(99).inspect}"
begin
  s.first(-1)
rescue ArgumentError => e
  puts "first negative: #{e.message}"
end

# `#last` computes from the quadruple instead, which is why a mixed
# sequence answers an Integer where its walk yields Floats.
puts "last: #{s.last}"
puts "last n: #{s.last(3).inspect}"
puts "last over: #{s.last(99).inspect}"
puts "last exclusive: #{(1...10).step(3).last}"
puts "last n exclusive: #{(1...10).step(3).last(2).inspect}"
puts "last mixed: #{(1..10.5).step(2).last}"
puts "last walked: #{(1..10.5).step(2).to_a.last}"
puts "last empty: #{(1..0).step(2).last.inspect}"
puts "last rational: #{Rational(1, 2).step(Rational(5, 2), Rational(1, 2)).last}"
begin
  s.last(-1)
rescue ArgumentError => e
  puts "last negative: #{e.message}"
end
begin
  (1..).step(2).last
rescue RangeError => e
  puts "last endless: #{e.message}"
end

# Equality reads the quadruple, so the call that built it does not enter
# into it -- but `#hash` is NOT held to that, exactly as in CRuby, because
# `2.hash` and `2.0.hash` differ.
puts "== same: #{(1..9).step(2) == 1.step(9, 2)}"
puts "eql? same: #{(1..9).step(2).eql?(1.step(9, 2))}"
puts "=== same: #{(1..9).step(2) === 1.step(9, 2)}"
puts "== float step: #{(1..10).step(2) == (1..10).step(2.0)}"
puts "hash float step: #{(1..10).step(2).hash == (1..10).step(2.0).hash}"
puts "hash same: #{(1..10).step(2).hash == (1..10).step(2).hash}"
puts "== exclusive: #{(1...10).step(2) == (1..10).step(2)}"
puts "== element: #{(1..10).step(2) === 3}"
puts "== other: #{(1..10).step(2) == 1}"

# `Array#[]` slices with a stride. A sequence is stricter than the
# equivalent Range: where `a[20..30]` answers nil, the sequence raises --
# except at a step of exactly 1, which routes through the Range slice.
a = (0..9).to_a
puts "aref: #{a[(1..10).step(2)].inspect}"
puts "aref endless: #{a[(0..).step(3)].inspect}"
puts "aref beginless: #{a[(..5).step(2)].inspect}"
puts "aref negative bounds: #{a[(-3..-1).step(2)].inspect}"
puts "aref descending: #{a[(9..0).step(-2)].inspect}"
puts "aref descending exclusive: #{a[(9...1).step(-2)].inspect}"
puts "aref against direction: #{a[(0..9).step(-2)].inspect}"
puts "aref exclusive: #{a[(0...9).step(3)].inspect}"
puts "aref float bounds: #{a[(1.5..5.5).step(2)].inspect}"
puts "aref float step: #{a[(0..9).step(2.9)].inspect}"
puts "aref numeric: #{a[1.step(7, 2)].inspect}"
puts "aref stride 1: #{a[(0..10).step(1)].inspect}"
puts "aref stride 1 past end: #{a[(11..12).step(1)].inspect}"
puts "aref slice: #{a.slice((1..7).step(2)).inspect}"
puts "aref at end: #{a[(10..11).step(2)].inspect}"
begin
  a[(0..10).step(3)]
rescue RangeError => e
  puts "aref out of range: #{e.message}"
end
begin
  a[(11..12).step(2)]
rescue RangeError => e
  puts "aref begin past end: #{e.message}"
end
begin
  a[(1..5).step(0.5)]
rescue ArgumentError => e
  puts "aref zero stride: #{e.message}"
end

# A zero step is rejected up front on both paths.
begin
  (1..10).step(0)
rescue ArgumentError => e
  puts "zero step: #{e.message}"
end
begin
  1.step(10, 0)
rescue ArgumentError => e
  puts "zero numeric step: #{e.message}"
end
# A non-numeric step is not refused until the walk needs it, so the
# blockless form is an ordinary Enumerator rather than a sequence.
puts "string step: #{(1..5).step("x").class}"
puts "string step inspect: #{(1..5).step("x").inspect}"
begin
  (1..5).step("x") { |x| x }
rescue TypeError => e
  puts "string step block: #{e.message}"
end

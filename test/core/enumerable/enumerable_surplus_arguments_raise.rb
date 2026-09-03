# The Enumerable/Array rows that take an OPTIONAL argument must still count
# what they were given: a surplus one is `wrong number of arguments`, not a
# conversion error on the first, and not silently ignored.
#
# Five rows were wrong. `tally(1, 2)` reported the first argument's type,
# `tally({}, 2)` and `each_slice(1, 2)`/`each_cons(1, 2)` ignored the extra
# entirely and answered, `min(1, 2)`/`max(1, 2)` did too, and `count(1, 2)`
# named the wrong bound ("expected 1" for a method whose range is 0..1).
#
# The rows that were already right are kept as controls, so a regression
# names which half broke.
rows = {
  "tally(1, 2)" => -> { [1, 2].tally(1, 2) },
  "tally(:x)" => -> { [1, 2].tally(:x) },
  "tally({}, 2)" => -> { [1, 2].tally({}, 2) },
  "sum(0, 1)" => -> { [1].sum(0, 1) },
  "min(1, 2)" => -> { [1].min(1, 2) },
  "max(1, 2)" => -> { [1].max(1, 2) },
  "first(1, 2)" => -> { [1].first(1, 2) },
  "uniq(1)" => -> { [1].uniq(1) },
  "flatten(1, 2)" => -> { [1].flatten(1, 2) },
  "count(1, 2)" => -> { [1].count(1, 2) },
  "zip()" => -> { [1].zip },
  "group_by(1)" => -> { [1].group_by(1) { |x| x } },
  "each_slice(1, 2)" => -> { [1].each_slice(1, 2) { break } },
  "each_cons(1, 2)" => -> { [1].each_cons(1, 2) { break } },
}
rows.each do |name, fn|
  r = begin
    v = fn.call
    "ok #{v.inspect}"
  rescue Exception => e
    "#{e.class}: #{e.message}"
  end
  puts "#{name}\t#{r}"
end
__END__
tally(1, 2)	ArgumentError: wrong number of arguments (given 2, expected 0..1)
tally(:x)	TypeError: no implicit conversion of Symbol into Hash
tally({}, 2)	ArgumentError: wrong number of arguments (given 2, expected 0..1)
sum(0, 1)	ArgumentError: wrong number of arguments (given 2, expected 0..1)
min(1, 2)	ArgumentError: wrong number of arguments (given 2, expected 0..1)
max(1, 2)	ArgumentError: wrong number of arguments (given 2, expected 0..1)
first(1, 2)	ArgumentError: wrong number of arguments (given 2, expected 0..1)
uniq(1)	ArgumentError: wrong number of arguments (given 1, expected 0)
flatten(1, 2)	ArgumentError: wrong number of arguments (given 2, expected 0..1)
count(1, 2)	ArgumentError: wrong number of arguments (given 2, expected 0..1)
zip()	ok [[1]]
group_by(1)	ArgumentError: wrong number of arguments (given 1, expected 0)
each_slice(1, 2)	ArgumentError: wrong number of arguments (given 2, expected 1)
each_cons(1, 2)	ArgumentError: wrong number of arguments (given 2, expected 1)

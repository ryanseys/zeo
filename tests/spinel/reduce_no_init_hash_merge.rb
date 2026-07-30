# reduce / inject with no initial value seeds the accumulator with the first
# element, so a block whose value has a different shape than that element --
# a Hash coming back out of #merge over a boxed element -- folds boxed.
p [{ a: 1 }, { a: 2 }].reduce { |x, y| x.merge(y) }
p [{ a: 1 }, { b: 2 }].inject { |x, y| x.merge(y) }
p [{ a: 1 }, { a: 2 }].reduce { |x, y| x.merge(y) { |_k, l, r| l + r } }
p [{ a: 1 }, { a: 2 }].reduce({}) { |x, y| x.merge(y) }

rows = [{ "n" => 1 }, { "n" => 2 }, { "m" => 3 }]
p rows.reduce { |x, y| x.merge(y) }
p rows.size

# an each-loop fold and a fold with an explicit seed still agree
acc = {}
rows.each { |r| acc = acc.merge(r) }
p acc

# arrays fold the same way
p [[1], [2], [3]].reduce { |x, y| x + y }
p [1, 2, 3].reduce { |x, y| x + y }

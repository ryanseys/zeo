p [{ a: 1 }, { b: 2 }, { a: 3 }].reduce({}) { |acc, hh| acc.merge(hh) { |_k, o, n| o + n } }
p [{ a: 1 }, { a: 2 }].reduce({}) { |acc, hh| acc.merge(hh) { |k, o, n| "#{k}:#{o + n}" } }
p [{ a: 1 }, { b: 2 }, { c: 3 }].reduce({}) { |acc, hh| acc.merge(hh) { |k, o, n| o + n } }
p [{ a: 1 }].reduce({}) { |acc, hh| acc.merge(hh) { |k, o, n| o + n } }
p [{ "a" => 1 }, { "b" => 2 }, { "a" => 3 }].reduce({}) { |acc, hh| acc.merge(hh) { |k, o, n| o + n } }
p({ a: 1 }.merge({ a: 3 }) { |k, o, n| o + n })
# empty-array reduce({}) returns the seed in the result's variant, not garbage
p [].reduce({}) { |acc, hh| acc.merge(hh) { |k, o, n| o + n } }
p [].reduce({}) { |a, b| a }
p [].reduce([]) { |a, b| a }
# array / hash valued conflict block (the value expr allocates)
p [{ a: 1 }, { a: "x" }].reduce({}) { |acc, hh| acc.merge(hh) { |k, o, n| [o, n] } }
p [{ a: 1 }, { a: 2 }].reduce({}) { |acc, hh| acc.merge(hh) { |k, o, n| { x: o, y: n } } }

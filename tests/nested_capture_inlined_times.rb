# An escaping closure can capture the param (and block-locals) of an INLINED
# `.times` block, even when that `.times` is itself nested inside another
# escaping block (Batch H). The `.times` body shares the enclosing Rust scope,
# so each iteration's param is cell-wrapped fresh -- matching Ruby's
# per-iteration block-param binding -- and the nested closure captures it.

# param of a `.times` nested inside an `.each` block, captured by a lambda
store = []
[1, 2].each do |a|
  store << ->() { a }
  2.times do |b|
    store << ->() { a + b }
  end
end
store.each { |p| puts p.call }

# a `.times` block-local (|i; n|) captured by a nested block
labels = []
[10].each do |base|
  3.times do |i; n|
    n = base + i
    labels << ->() { n }
  end
end
labels.each { |p| puts p.call }

# the uncaptured fast path is untouched: break still works
total = 0
5.times { |i| total += i }
puts total
r = 5.times { |i| break i * 2 if i == 3 }
p r

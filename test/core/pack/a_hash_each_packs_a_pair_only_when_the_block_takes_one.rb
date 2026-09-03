# `Hash#each` yields two values to a block that can take two, and packs them
# into one array only for a block that cannot -- `rb_hash_foreach`'s own rule.
# Every shape below decides that question differently, and the lambda case is
# load-bearing: CRuby raises ArgumentError there PRECISELY because a lambda
# receives the packed pair as one argument.
h = { a: 1, b: 2 }
out = []
h.each { |pair| out << [:one, pair] }
h.each { |k, v| out << [:two, k, v] }
h.each { |*a| out << [:splat, a] }
h.each { |k, *r| out << [:k_rest, k, r] }
h.each { |k, v, *r| out << [:kv_rest, k, v, r] }
h.each { out << [:none] }
h.each { |k, v, w| out << [:three, k, v, w] }
out.each { |r| p r }
p h.map { |k, v| "#{k}#{v}" }
p h.map { |pair| pair.class }
p h.select { |k, v| v > 1 }
p h.count { |k, v| v > 0 }
p h.each_with_object([]) { |(k, v), acc| acc << k }
p h.to_a
begin
  h.each(&->(k, v) { out << [:lambda, k, v] })
  p :lambda_ok
rescue => e
  p [:lambda_err, e.class]
end
p out.last
begin
  p h.each(&:class)
rescue => e
  p [:sym_err, e.class]
end
__END__
[:one, [:a, 1]]
[:one, [:b, 2]]
[:two, :a, 1]
[:two, :b, 2]
[:splat, [[:a, 1]]]
[:splat, [[:b, 2]]]
[:k_rest, :a, [1]]
[:k_rest, :b, [2]]
[:kv_rest, :a, 1, []]
[:kv_rest, :b, 2, []]
[:none]
[:none]
[:three, :a, 1, nil]
[:three, :b, 2, nil]
["a1", "b2"]
[Array, Array]
{b: 2}
2
[:a, :b]
[[:a, 1], [:b, 2]]
[:lambda_err, ArgumentError]
[:three, :b, 2, nil]
{a: 1, b: 2}

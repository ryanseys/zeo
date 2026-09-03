h = { a: 1, b: 2, c: 3 }
p h.values_at(:a, :c)
p h.assoc(:b)
p h.rassoc(2)
p({ a: 1, b: nil, c: 3 }.compact)
y = { a: 1, b: 2, c: 3 }; p y.shift; p y
p({ a: 1, b: 2 }.select! { |_k, v| v > 1 })
z = { a: 1, b: 2 }; z.transform_values! { |v| v * 10 }; p z
p({ a: 1, b: 2 } <= { a: 1, b: 2, c: 3 })
p({ a: 1, b: 2, c: 3 } > { a: 1 })
__END__
[1, 3]
[:b, 2]
[:b, 2]
{a: 1, c: 3}
[:a, 1]
{b: 2, c: 3}
{b: 2}
{a: 10, b: 20}
true
true

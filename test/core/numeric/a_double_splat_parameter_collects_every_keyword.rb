# Keywords whose values are arrays, hashes, strings and symbols, and a call
# with none.
# (spinel issue #3111)
def f(**k); p k; end
f(value: ["x"])
f(value: {x: "x"})
f(a: 1, b: [2, 3])
f(name: "n", tags: [:a, :b])
f()
__END__
{value: ["x"]}
{value: {x: "x"}}
{a: 1, b: [2, 3]}
{name: "n", tags: [:a, :b]}
{}

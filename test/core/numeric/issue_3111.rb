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

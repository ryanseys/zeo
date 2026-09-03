p(->((a, b)) {}.parameters)
p(proc { |(a, b)| }.parameters)
p(->(x, (a, b)) {}.parameters)
p(->((a, b)) {}.call([1, 2]))
p(->(x) {}.call(3))
p(->(x) { x }.parameters)
p(->((a, b)) { a + b }.call([1, 2]))
p(proc { |(a, b)| b }.call([3, 4]))
__END__
[[:req]]
[[:opt]]
[[:req, :x], [:req]]
nil
nil
[[:req, :x]]
3
4

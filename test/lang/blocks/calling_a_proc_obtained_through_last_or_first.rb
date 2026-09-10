# `pipe.last.call(6)` -- the index read is the receiver of the call.
pipe = [->(x) { x * 2 }, ->(x) { x + 1 }]
p pipe.last.call(6)
p pipe.first.call(6)
fns = [->(a, b) { a + b }, ->(a, b) { a * b }]
p fns.last.call(3, 4)
__END__
7
12
12

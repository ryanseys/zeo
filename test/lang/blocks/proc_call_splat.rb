pr = ->(a, b, c) { a + b + c }
args = [1, 2, 3]
p(pr.call(*args))
p(pr[*args])
p(pr.(*args))
p(pr.yield(*args))
s = ->(x, y) { "#{x}-#{y}" }
p(s.call(*["a", "b"]))
add = proc { |a, b| a + b }
p(add.call(*[4, 5]))
__END__
6
6
6
6
"a-b"
9

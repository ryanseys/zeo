# `&block` passed down both recursive calls of a towers-of-hanoi solver, whose
# yields collect in the caller's array.
# (spinel issue #3166)
def hanoi(n, from, to, via, &block)
  return if n.zero?
  hanoi(n - 1, from, via, to, &block)
  yield [from, to]
  hanoi(n - 1, via, to, from, &block)
end

moves = []
hanoi(2, :a, :c, :b) { |m| moves << m }
p moves
__END__
[[:a, :b], [:a, :c], [:b, :c]]

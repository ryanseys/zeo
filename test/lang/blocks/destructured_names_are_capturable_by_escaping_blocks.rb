# A destructured name is an ordinary local: an escaping block captures it by
# reference, so a write inside the block is visible after it returns. This
# regressed once already -- `captures::own_param_names` kept its own copy of
# the param-name walk and didn't know about destructures, so the name was
# classified as a block-own local and silently read `nil`.

def capture((a, b))
  bump = -> { a += 10 }
  bump.call
  [a, b]
end
p capture([1, 2])

def collect((x, y))
  out = []
  [1, 2].each { |i| out << (x * i + y) }
  out
end
p collect([10, 1])
__END__
[11, 2]
[11, 21]

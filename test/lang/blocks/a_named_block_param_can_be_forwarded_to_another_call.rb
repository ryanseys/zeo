# `c` is a `New`-assigned LOCAL inside `go`, not a parameter or an
# implicit-self call target -- see the self-capture test's comment for
# why (both are separate, pre-existing, unrelated gaps).

class Collector
  def each_num(a, b)
    yield a
    yield b
  end
end
class Relay
  def go(a, b, &blk)
    c = Collector.new
    c.each_num(a, b, &blk)
  end
end
total = 0
Relay.new.go(3, 4) { |n| total += n }
puts total
__END__
7

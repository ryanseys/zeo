# The original blanket rejection of Proc-within-Proc was LIFTED
# (`Thread.new { m.synchronize { } }` is the canonical threading
# idiom): method-level captures are shared cells that compose through
# any nesting depth. This exact snippet was that rejection's own
# negative test -- now a positive one, oracle-verified.

class Collector
  def each_num(a, b)
    yield a
    yield b
  end
end
c = Collector.new
c.each_num(1, 2) { |n| c.each_num(n, n) { |m| puts m } }
__END__
1
1
2
2

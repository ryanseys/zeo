# `c` is a `New`-assigned LOCAL, not a method parameter -- see
# `an_escaping_block_can_mutate_an_ivar_via_self_capture`'s comment for
# why (params are always statically `Poly`, a separate pre-existing
# gap unrelated to this test's actual point: `return` inside a real
# escaping Proc unwinding all the way out of `find_even`, not just the
# block/`.each_num` call).

class Collector
  def each_num(a, b, c)
    yield a
    yield b
    yield c
  end
end
class Finder
  def find_even
    c = Collector.new
    c.each_num(1, 3, 4) { |n| return n if n % 2 == 0 }
    -1
  end
end
puts Finder.new.find_even
__END__
4

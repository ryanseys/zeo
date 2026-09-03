# The INNER block reads the OUTER block's own param `n`. The outer
# block promotes `n` to a shared `Arc<Mutex>` cell (see
# `analyze::captures` and `clif::blocks::build_proc`), so the
# inner closure captures it -- previously a clean rejection,
# now the real Ruby behavior. Oracle: n=1 -> 3+1,4+1; n=2 -> 3+2,4+2.

class Collector
  def each_num(a, b)
    yield a
    yield b
  end
end
c = Collector.new
c.each_num(1, 2) { |n| c.each_num(3, 4) { |m| puts m + n } }
__END__
4
5
5
6

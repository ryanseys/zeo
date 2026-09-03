# `next` skips to the next yield; `break value` makes the WHOLE call
# (`each_num(...)`) evaluate to that value, stopping further yields.

class Collector
  def each_num(a, b, c)
    yield a
    yield b
    yield c
  end
end
result = Collector.new.each_num(1, 2, 3) { |n| next if n == 2; break "stopped" if n == 3; puts n }
puts result
__END__
1
stopped

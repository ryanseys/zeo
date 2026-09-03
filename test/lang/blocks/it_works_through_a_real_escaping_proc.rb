class Collector
  def each_num(a, b)
    yield a
    yield b
  end
end
Collector.new.each_num(7, 8) { puts it + 1 }
__END__
8
9

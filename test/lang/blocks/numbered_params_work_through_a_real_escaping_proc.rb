class Collector
  def each_num(a, b)
    yield a
    yield b
  end
end
Collector.new.each_num(5, 6) { puts _1 * 2 }
__END__
10
12

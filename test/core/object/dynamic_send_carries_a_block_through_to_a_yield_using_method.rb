class Collector
  def each_num(a, b, c)
    yield a
    yield b
    yield c
  end
end
total = 0
Collector.new.send(:each_num, 1, 2, 3) { |n| total += n }
puts total
__END__
6

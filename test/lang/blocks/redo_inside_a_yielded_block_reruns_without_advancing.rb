class Collector
  def each_num(a, b)
    yield a
    yield b
  end
end
attempts = 0
Collector.new.each_num(1, 2) do |n|
  attempts += 1
  if n == 1 && attempts < 2
    redo
  end
  puts "n=#{n} attempts=#{attempts}"
end
__END__
n=1 attempts=2
n=2 attempts=3

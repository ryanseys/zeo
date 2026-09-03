class Counter
  def initialize
    @count = 0
  end
  def increment
    @count += 1
  end
  alias inc increment
  def value
    @count
  end
end
c = Counter.new
c.inc
c.inc
c.increment
puts c.value
__END__
3

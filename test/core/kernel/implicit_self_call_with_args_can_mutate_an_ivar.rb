class Counter
  def initialize
    @count = 0
  end

  def bump(n)
    add(n)
    @count
  end

  def add(n)
    @count = @count + n
  end
end

c = Counter.new
puts c.bump(5)
puts c.bump(2)
__END__
5
7

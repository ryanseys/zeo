puts eval("1 + 2")

x = 1
eval("x = x + 1")
puts x

class Box
  def initialize
    eval("@value = 10")
  end

  def value
    @value
  end
end

b = Box.new
puts b.value
__END__
3
2
10

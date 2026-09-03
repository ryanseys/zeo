class Adder
  def add3(a, b, c)
    a + b + c
  end
end
a = Adder.new
arr = [1, 2, 3]
puts a.add3(*arr)
puts a.add3(1, *[2, 3])
__END__
6
6

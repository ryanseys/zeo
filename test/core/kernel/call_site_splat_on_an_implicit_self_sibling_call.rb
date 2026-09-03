class Adder
  def add3(a, b, c)
    a + b + c
  end
  def run(arr)
    add3(*arr)
  end
end
puts Adder.new.run([1, 2, 3])
__END__
6

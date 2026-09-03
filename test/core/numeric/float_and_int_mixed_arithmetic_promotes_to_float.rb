class Adder
  def add(a, b)
    a + b
  end
end
puts Adder.new.add(1, 2.5)
puts Adder.new.add(2.5, 1)
__END__
3.5
3.5

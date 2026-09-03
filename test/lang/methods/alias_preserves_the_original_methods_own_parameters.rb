class Calc
  def add(a, b)
    a + b
  end
  alias sum add
end
puts Calc.new.sum(3, 4)
__END__
7

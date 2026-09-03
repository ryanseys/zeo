module MathHelpers
  def double(x)
    x * 2
  end
end
class Calc
  extend MathHelpers
end
puts Calc.double(21)
__END__
42

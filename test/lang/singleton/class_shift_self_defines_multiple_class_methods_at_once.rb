class MathUtils
  class << self
    def square(x)
      x * x
    end
    def cube(x)
      x * x * x
    end
  end
end
puts MathUtils.square(4)
puts MathUtils.cube(3)
__END__
16
27

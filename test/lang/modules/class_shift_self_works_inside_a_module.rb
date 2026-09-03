module MyMath
  class << self
    def double(x)
      x * 2
    end
  end
end
puts MyMath.double(5)
__END__
10

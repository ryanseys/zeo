class Box
  def initialize(v)
    @v = v
  end

  def value
    @v
  end

  def describe
    self.value
  end

  def identity
    self
  end
end

b = Box.new(42)
puts b.describe
puts b.identity.value
__END__
42
42

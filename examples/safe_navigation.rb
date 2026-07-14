class Box
  def initialize(value)
    @value = value
  end
  def value
    @value
  end
end

b = Box.new(:present)
puts(b&.value)

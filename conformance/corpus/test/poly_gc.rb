class Trash
  def initialize(n)
    @n = n
    @s = "padding payload " * 64
  end
end

class Holder
  def initialize(value)
    @value = value
  end

  attr_reader :value
end

def make_poly_array
  value = "array-" + 123.to_s
  [value, 1]
end

def make_holder
  value = "ivar-" + 456.to_s
  Holder.new(value)
end

values = make_poly_array
holder = make_holder
Holder.new(1)

junk = []
i = 0
while i < 5000
  junk << Trash.new(i)
  i = i + 1
end

puts junk.length
puts values[0]
puts holder.value

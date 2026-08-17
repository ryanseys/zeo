module DSL
  def hello
    "hi"
  end

  def twice(x)
    x * 2
  end
end

module More
  def bye
    "bye"
  end
end

extend DSL
extend More

puts hello
p twice(3)
p bye
p [hello, bye].join(" ")

def wrapper
  hello
end

p wrapper

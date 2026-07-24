# A method whose return paths mix a genuine int (an int literal or a
# param-sourced int ivar) with a string must box its return to poly so
# the int arm is not silently returned through the string pointer slot.
class Record
  def initialize(id, name)
    @id = id
    @name = name
  end

  # Both arms are ivar reads; the unify picks "string", hiding the int
  # @id arm inside the tail IfNode.
  def [](field)
    if field == 0
      @id
    else
      @name
    end
  end
end

r = Record.new(7, "hi")
puts r[0]
puts r[1]
p r[0]
p r[1]

# Int literal buried in a branch, string in the other.
class Tagged
  def initialize(s)
    @s = s
  end

  def value(numeric)
    if numeric
      100
    else
      @s
    end
  end
end

t = Tagged.new("text")
puts t.value(true)
puts t.value(false)
p t.value(true)
p t.value(false)

# The exact reported shape: a case/when field lookup over literal-init
# ivars of mixed types.
class Row
  def initialize
    @id = 7
    @name = "hi"
  end

  def [](field)
    case field
    when :id then @id
    when :name then @name
    end
  end
end

p Row.new[:id]
p Row.new[:name]

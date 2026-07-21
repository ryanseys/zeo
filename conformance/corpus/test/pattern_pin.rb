# Pattern matching: PinnedExpressionNode + PinnedVariableNode.
#
# Pin patterns match the scrutinee by `==` against the value of an
# existing local / ivar / arbitrary expression -- they do not bind
# new variables. Both prism node types (PinnedVariableNode for `^var`
# and PinnedExpressionNode for `^(expr)`) are routed through the
# shared `@nd_expression` slot so the codegen treats them uniformly.

# --- Pin against int local
x = 42
case 42
in ^x
  puts "pin int match"
in 0
  puts "zero"
end

# --- Pin miss falls through to next arm / else
y = 10
case 42
in ^y
  puts "pin y match"
else
  puts "no pin y"
end

# --- Pin against string local
s = "hello"
case "hello"
in ^s
  puts "pin str match"
end

# Pin string miss
t = "goodbye"
case "hello"
in ^t
  puts "pin t"
else
  puts "no pin t"
end

# --- Pin against arbitrary expression `^(expr)`
val = 7
case 21
in ^(val * 3)
  puts "pin expr 21"
else
  puts "miss"
end

# Pin expression with method call
def double(n)
  n * 2
end
case 14
in ^(double(7))
  puts "pin call 14"
end

# --- Pin against symbol
sym = :foo
case :foo
in ^sym
  puts "pin sym match"
end

# Symbol miss
case :bar
in ^sym
  puts "pin sym bar"
else
  puts "no sym bar"
end

# --- Pin works alongside literal arms in same case
mode = :verbose
case :verbose
in :quiet
  puts "q"
in ^mode
  puts "pin mode hit"
in :other
  puts "o"
end

# --- Pin against ivar
class Box
  def initialize(v)
    @v = v
  end

  def matches(x)
    case x
    in ^@v
      "ivar match"
    else
      "no match"
    end
  end
end

b = Box.new(99)
puts b.matches(99)
puts b.matches(100)

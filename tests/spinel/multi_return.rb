def divmod(a, b)
  [a / b, a % b]
end

q, r = divmod(17, 5)
puts q  # 3
puts r  # 2

# Swap
x = 10
y = 20
x, y = y, x
puts x  # 20
puts y  # 10

# From array literal
a, b, c = [100, 200, 300]
puts a  # 100
puts b  # 200
puts c  # 300

# Multiple return values used in expressions
def minmax(a, b)
  if a < b
    [a, b]
  else
    [b, a]
  end
end

lo, hi = minmax(42, 7)
puts lo  # 7
puts hi  # 42

puts "done"

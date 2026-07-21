# Bigint `**`, `<<` and `>>` keep arbitrary precision. `b` is promoted to
# bigint by `b = b * 2` in the loop; the power and the shifts must not fall
# back to the int path (which would truncate the receiver to 64 bits).
b = 1
i = 0
while i < 70
  b = b * 2
  i = i + 1
end

puts b
puts(b ** 2)
puts(b << 5)
puts(b >> 3)
# negative shifts route through the opposite primitive
puts(b << -3)
puts(b >> -5)

# a down-shift that lands back inside small-int range
c = 1
j = 0
while j < 65
  c = c * 2
  j = j + 1
end
puts(c >> 60)

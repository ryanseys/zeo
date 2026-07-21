a = 0
b = 1
i = 0
while i < 1000
  c = a + b
  a = b
  b = c
  i = i + 1
end
puts a

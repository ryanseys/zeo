i = 0
sum = 0
while i < 10
  i += 1
  next if i == 5
  break if i == 8
  sum += i
end
puts sum
puts i

n = 5
until n == 0
  n -= 1
end
puts n

count = 0
count += 1 while count < 3
puts count

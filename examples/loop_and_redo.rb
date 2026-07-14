i = 0
result = loop do
  i += 1
  break i * 10 if i == 3
end
puts result

j = 0
attempts = 0
while j < 3
  attempts += 1
  if attempts < 5 && j == 1
    redo
  end
  j += 1
end
puts j
puts attempts

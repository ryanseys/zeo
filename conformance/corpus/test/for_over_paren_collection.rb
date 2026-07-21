# A parenthesized collection in a for loop must iterate like the bare form.
sum = 0
for i in (0..4)
  sum += i
end
p sum

for j in (1...4)
  p j
end

total = 0
for element in ([5, 6, 7])
  total += element
end
p total

for key, value in ({ "a" => 1, "b" => 2 })
  puts "#{key}=#{value}"
end

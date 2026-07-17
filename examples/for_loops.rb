sum = 0
for i in 1..5
  sum += i
end
puts sum
puts i

arr = [10, 20, 30]
for el in arr
  puts el
end

# Over a hash, `for k, v` destructures each pair; `for pair` takes it whole.
total = 0
for k, v in { "a" => 1, "b" => 2, "c" => 3 }
  total += v
  puts "#{k}:#{v}"
end
p total

for pair in { x: 10, y: 20 }
  p pair
end

# A parenthesized range or array iterates just like the bare form.
psum = 0
for n in (1..3)
  psum += n
end
p psum

for e in ([9, 8])
  p e
end

# A `for` loop evaluates to the collection it iterated (unless the body
# `break`s with a value).
p(for x in [1, 2, 3]; end)     # [1, 2, 3]
p(for x in [1, 2]; break :done; end)   # :done

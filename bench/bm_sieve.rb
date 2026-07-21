num = 500
count = 0
flags0 = Array.new(8192, 1)
k = 0
while k < num
  k = k + 1
  count = 0
  flags = Array.new(8192, 1)
  i = 2
  while i < 8192
    i = i + 1
    if flags[i] == 1
      j = i * i
      while j < 8192
        j = j + i
        flags[j] = 0
      end
      count = count + 1
    end
  end
end
puts count
puts "done"

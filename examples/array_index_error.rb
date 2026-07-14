a = [1, 2, 3]
begin
  a[-10] = :x
rescue IndexError => e
  puts "caught out-of-range index assignment"
end
a[5] = :y
puts a.length
puts a[5]

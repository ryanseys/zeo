a = [1, 2, 3].freeze
puts a.frozen?
begin
  a << 4
rescue FrozenError
  puts "array << raised"
end
begin
  a[0] = 9
rescue FrozenError
  puts "array []= raised"
end
begin
  a.push(5)
rescue FrozenError
  puts "array push raised"
end
puts a.inspect

b = [1, 2, 3]
puts b.frozen?
b << 4
b[0] = 9
puts b.inspect

s = ["x", "y"].freeze
begin
  s << "z"
rescue FrozenError
  puts "str_array << raised"
end
puts s.inspect

h = {a: 1}.freeze
puts h.frozen?
begin
  h[:b] = 2
rescue FrozenError
  puts "hash []= raised"
end

g = {a: 1}
puts g.frozen?
g[:b] = 2
puts g.length

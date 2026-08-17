{ a: 1 }.each do |k, v|
  next
  puts "unreachable"
end
puts "ok"

h = { a: 1, b: 2, c: 3 }
seen = []
h.each { |k, v| next if k == :b; seen << k }
p seen

h.each_pair { |k, v| next if v == 1; seen << v }
p seen

sums = 0
{ "x" => 1, "y" => 2 }.each { |k, v| next if k == "x"; sums += v }
p sums

d = { a: 1, b: 2, c: 3 }
d.each { |k, v| d.delete(k) if k == :a }
p d

r = ({ a: 1, b: 2 }.each { |k, v| break k if v == 2 })
p r

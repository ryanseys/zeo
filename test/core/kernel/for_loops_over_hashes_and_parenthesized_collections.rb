total = 0
for k, v in { "a" => 1, "b" => 2, "c" => 3 }
  total += v
  puts "#{k}:#{v}"
end
p total
for pair in { x: 10, y: 20 }
  p pair
end
for k, v in {}
  puts "unreachable"
end
sum = 0
for i in (1..3)
  sum += i
end
p sum
for e in ([9, 8])
  p e
end
for last in [7, 8, 9]
end
p last
__END__
a:1
b:2
c:3
6
[:x, 10]
[:y, 20]
6
9
8
9

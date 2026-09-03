puts [1, [2, nil]], "x"
puts
r = p 1, "two"
p r
r2 = p 5
p r2
print "a", 1, "\n"
puts format("%s scored %05.1f%%", "Bob", 92.5)
printf("%d-%x\n", 255, 255)
srand(42)
v = rand(10)
puts v.between?(0, 9)
puts rand.between?(0.0, 1.0)
puts rand(10).class
old = srand(7)
puts old
caught = catch(:done) do
  [1, 2, 3].each { |i| throw :done, i * 10 if i == 2 }
  :never
end
p caught
p(catch(:t) { 5 })
__END__
1
2

x

1
"two"
[1, "two"]
5
5
a1
Bob scored 092.5%
255-ff
true
true
Integer
42
20
5

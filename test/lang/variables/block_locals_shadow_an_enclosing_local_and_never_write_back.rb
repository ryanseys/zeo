sum = 99
[1].each { |x; sum| sum = x }
p sum

a = 1
b = 2
[0].each { |z; a, b| a = 7; b = 8 }
p [a, b]

n = 100
[10, 20].each { |v; n| n = v }
p n

r = 0
[5].each do |i|
  [9].each { |j; r| r = j }
  r = i
end
p r
__END__
99
[1, 2]
100
5

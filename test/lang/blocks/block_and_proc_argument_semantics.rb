h = { a: 1, b: 2 }
h.each { |pair| p pair }
h.each { |k, v| puts "#{k}=#{v}" }
def show(pair) = p pair
h.each(&method(:show))

add = proc { |a:, b:| a + b }
p add.call(a: 1, b: 2)
opt = proc { |x:, y: 100| [x, y] }
p opt.call(x: 5)
begin
  add.call(a: 1)
rescue ArgumentError => e
  puts e.message
end

lam = ->(x:, y: 9) { [x, y] }
p lam.call(x: 5)

pr = proc { |a, b| [a, b] }
p pr.call(1, 2, 3)
p pr.call(1)
p [[1, 2], [3, 4]].map { |a, b| a + b }
__END__
[:a, 1]
[:b, 2]
a=1
b=2
[:a, 1]
[:b, 2]
3
[5, 100]
missing keyword: :b
[5, 9]
[1, 2]
[1, nil]
[3, 7]

a = [1, 2, 3]
b = [4, 5, 6]

p a[0]
p b[0]
a.each { |x| print x }
puts

def a.[](i) = -1

def a.each
  yield 99
  self
end

p a[0]
p b[0]
a.each { |x| print x }
puts
b.each { |x| print x }
puts

class Wrapped < Array
end
w = Wrapped.new([7, 8])
p w[1]
p a[1]

module Shout
  def [](i) = 1000 + i
end
b.extend(Shout)
p b[1]
p a[1]
p Wrapped.new([9])[0]
__END__
1
4
123
-1
4
99
456
8
-1
1001
-1
9

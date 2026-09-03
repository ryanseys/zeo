# `super(first, *rest)`, `super(**opts)`, and `super(*args, &block)` all
# forward through the runtime arg vector -- the splat/double-splat/block-
# pass shapes a plain call carries, now available at a `super` site too.

class A
  def m(a, b, c) = "#{a}-#{b}-#{c}"
  def kw(x:, y:) = "#{x}/#{y}"
  def each(*a) = a.each { |v| yield v }
end
class B < A
  def m(first, *rest) = super(first, *rest)
  def kw(**opts) = super(**opts)
  def each(*args, &block) = super(*args, &block)
end
puts B.new.m(1, 2, 3)
puts B.new.kw(x: 7, y: 9)
B.new.each(10, 20) { |v| puts v }
__END__
1-2-3
7/9
10
20

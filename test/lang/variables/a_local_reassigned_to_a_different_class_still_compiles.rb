# `local_types` is read flow-insensitively, so a retyped local must
# widen to Poly rather than let a later class's identifier describe an
# earlier read.

class A
  def who = "A"
end
class B
  def who = "B"
end
x = A.new
puts x.who
x = B.new
puts x.who
y = 1
y = "str"
puts y
__END__
A
B
str

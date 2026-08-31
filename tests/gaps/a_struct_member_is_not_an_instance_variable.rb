S = Struct.new(:a, :b)
class S
  def peek = @a
  def poke = @a = 99
end
s = S.new(1, 2)
p s.peek
p s.instance_variables
s.poke
p [s.a, s.instance_variables, s.instance_variable_get(:@a)]

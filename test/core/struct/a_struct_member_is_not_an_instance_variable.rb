# A Struct member is not an instance variable. `@a` reads nil even though
# `a` is 1, `instance_variables` is empty until a method assigns `@a`, and
# assigning it adds a real ivar without touching the member.
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
__END__
nil
[]
[1, [:@a], 99]

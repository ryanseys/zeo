# get/set/list over a concrete receiver, a poly receiver, missing names,
# an empty object, and a malformed-name NameError.

class Box
  def initialize(v); @v = v; @tag = "b"; end
end
b = Box.new(7)
p b.instance_variables
p b.instance_variable_get(:@v)
b.instance_variable_set(:@v, 70)
p b.instance_variable_get("@v")
p b.instance_variable_get(:@missing)
class Bare; end
p Bare.new.instance_variables
def peek(o) = o.instance_variable_get(:@v)
p peek(Box.new(99))
begin
  b.instance_variable_get(:v)
rescue NameError => e
  puts e.message
end
__END__
[:@v, :@tag]
7
70
nil
[]
99
'v' is not allowed as an instance variable name

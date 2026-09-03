# A generated WRITER's reported parameter, which differs by where it came from.
#
# ruby names an `attr_writer`/`attr_accessor` slot NOTHING -- the C accessor
# entry declares only a count -- and names a Struct member's slot `_`. zeo
# spells both as the same generated accessor, so the struct half renames its
# slot after lowering; the mark that elides the frame survives either way.

class Plain
  attr_accessor :a
  attr_writer :b
  attr_reader :c
end
p Plain.instance_method(:a=).parameters
p Plain.instance_method(:a).parameters
p Plain.instance_method(:b=).parameters
p Plain.instance_method(:c).parameters
p Plain.instance_method(:a=).arity

S = Struct.new(:x, :y)
p S.instance_method(:x=).parameters
p S.instance_method(:x).parameters
p S.instance_method(:x=).arity

# A struct minted where the compiler cannot fold it takes the same answers.
def mint(*names) = Struct.new(*names)
T = mint(:z)
p T.instance_method(:z=).parameters

D = Data.define(:d)
p D.instance_method(:d).parameters

# The writers still WORK, and still write the slot they name.
o = Plain.new
o.a = 1
o.b = 2
p [o.a, o.instance_variable_get(:@b)]
s = S.new(1, 2)
s.x = 9
p [s.x, s.to_a]
__END__
[[:req]]
[]
[[:req]]
[]
1
[[:req, :_]]
[]
1
[[:req, :_]]
[]
[1, 2]
[9, [9, 2]]

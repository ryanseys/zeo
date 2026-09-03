# An ivar named inside `class << X` belongs to X's SINGLETON CLASS, which is a
# different object from X. Both spellings of the body have to agree.

def try(tag)
  puts "#{tag}: #{yield.inspect}"
rescue StandardError => e
  puts "#{tag}: #{e.class}: #{e.message}"
end

# a: per-object form, plain write.
A = Module.new
class << A
  @plain = 7
end
try("a") { [A.singleton_class.instance_variable_get(:@plain), A.instance_variable_get(:@plain)] }

# b: the `class << self` form, which already agreed.
class B
  class << self
    @plain = 7
  end
end
try("b") { [B.singleton_class.instance_variable_get(:@plain), B.instance_variable_get(:@plain)] }

# c: read back inside the same body.
C = Module.new
class << C
  @seen = 3
  @echo = @seen
end
try("c") { C.singleton_class.instance_variable_get(:@echo) }

# d: an operator-assign and an or-assign, which read before they write.
D = Module.new
class << D
  @n = 1
  @n += 1
  @m ||= "set"
  @m ||= "not this"
end
try("d") { [D.singleton_class.instance_variable_get(:@n), D.singleton_class.instance_variable_get(:@m)] }

# e: an unset ivar reads nil, not an error.
E = Module.new
class << E
  @out = @never_set.inspect
end
try("e") { E.singleton_class.instance_variable_get(:@out) }

# f: the accessor written BESIDE it reads the object's own ivars, so it does
#    not see what the body wrote -- oracle-verified, and the reason the write
#    cannot simply stay where it stands.
F = Module.new
class << F
  @slack = "x"
  attr_accessor :slack
end
try("f") { F.slack }

# g: two objects do not share.
G1 = Module.new
G2 = Module.new
class << G1
  @who = "g1"
end
class << G2
  @who = "g2"
end
try("g") { [G1.singleton_class.instance_variable_get(:@who), G2.singleton_class.instance_variable_get(:@who)] }

# i: instance_variables reports it on the singleton class.
I = Module.new
class << I
  @a = 1
  @b = 2
end
try("i") { I.singleton_class.instance_variables.sort }

# The `class << self` spelling, whose nested reads were wrong in the same way:
# only the two bare statement forms used to be mapped.
class J
  class << self
    @seen = 3
    @echo = @seen
    @n = 1
    @n += 1
  end
end
try("j") { [J.singleton_class.instance_variable_get(:@echo), J.singleton_class.instance_variable_get(:@n)] }

# A `def` body's ivars belong to whatever `self` it runs against, so the
# rewrite must stop at the definition boundary.
K = Module.new
class << K
  @outer = "singleton"
  def peek = @outer
end
try("k") { [K.peek, K.singleton_class.instance_variable_get(:@outer)] }
__END__
a: [7, nil]
b: [7, nil]
c: 3
d: [2, "set"]
e: "nil"
f: nil
g: ["g1", "g2"]
i: [:@a, :@b]
j: [3, 2]
k: [nil, "singleton"]

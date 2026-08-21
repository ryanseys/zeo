# An unresolvable constant raises NameError, and in a value position its raise
# carries an sp_Class the slot cannot hold, so the tail assignment casts it to
# the slot's own zero. The list of node kinds that got that treatment named the
# bare spelling and not the qualified one, so a rescued `M::Missing` in a value
# position assigned an sp_Class to a `const char *` and did not build at all --
# a program CRuby runs and answers.
r = begin
  NoSuchMod::NoSuchConstF
rescue NameError
  "qualified"
end
p r

# the same in an Integer-typed slot, and in an object-typed one
n = begin
  Absent::COUNT
rescue NameError
  7
end
p n

class Frame
  attr_reader :n
  def initialize(n); @n = n; end
end
f = begin
  Gone::FRAME
rescue NameError
  Frame.new(3)
end
p f.n

# a deeper path, and one whose parent is a real module
module Real; end
d = begin
  Real::Missing::Deep
rescue NameError
  "deep"
end
p d

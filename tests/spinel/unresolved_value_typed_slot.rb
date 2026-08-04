# An unresolved name lowers to a raising token whose C value is boxed. Landing
# it in a concretely-typed slot needs the coercing emit, or the emitted C
# assigns an sp_RbVal to the slot's own type and the program does not build at
# all. Two slots the coercion was not reaching: a `const char *` operand of a
# string concat (the token arrived wrapped in a paren, so the match missed it)
# and an object-pointer ivar written through an attr writer.
class Frame
  attr_reader :n
  def initialize(n); @n = n; end
end

class Holder
  attr_accessor :frame
  def initialize; @frame = Frame.new(1); end
end

# concat: the unresolved call is the right operand of `+`, written with the
# parentheses that hid the raising token from the coercion's text match
def concat_unresolved(a, b)
  "" + (Missing.build(a.to_f, b.to_i))
end

begin
  puts concat_unresolved("1.5", "2")
rescue NameError => e
  p e.class
end

def concat_unresolved_bare(a)
  "" + Absent.build(a.to_i)
end

begin
  puts concat_unresolved_bare("3")
rescue NameError => e
  p e.class
end

# object-pointer slot: an attr writer taking an unresolved name
h = Holder.new
p h.frame.n
begin
  h.frame = missing_frame
rescue NameError => e
  p e.class
end
p h.frame.n

# a resolvable value still goes into the same slot unchanged
h.frame = Frame.new(7)
p h.frame.n
p ("" + "ok")

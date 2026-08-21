# An accessor call on this body's OWN class is replaced by the ivar access
# itself -- no dispatch, no trampoline, no frame -- which is what the rustc
# backend has always done at a statically-typed receiver.
#
# What must NOT fold, and why each is a question only the run time answers:
#   - a hand-written accessor under TracePoint or line coverage (it has a
#     body an instrumented run has to be able to observe)
#   - an argument count the accessor does not take (ArgumentError)
#   - a frozen receiver on the writer (FrozenError, from the write itself)
class C
  attr_accessor :a
  attr_reader :b
  def initialize = (@a = 1; @b = 2)
  def read_a = a
  def read_self_a = self.a
  def write_a(v) = (self.a = v)
  def read_b = b
  def hand_written = @a * 10
  def bad_arity = (a(1) rescue "ArgumentError: #{$!.message}")
end

c = C.new
p [c.read_a, c.read_self_a, c.read_b]
p c.write_a(9)
p [c.a, c.read_a]
p c.hand_written
p c.bad_arity

d = C.new.freeze
begin
  d.write_a(1)
rescue FrozenError => e
  puts e.message.sub(/0x[0-9a-f]+/, "0xADDR")
end

# An inherited accessor read from a subclass body resolves through the
# chain and folds against the SUBCLASS's own layout.
class D < C
  def peek = a
  def poke(v) = (self.a = v)
end
p D.new.peek
p D.new.poke(7)

# A writer answers the value ASSIGNED, whatever the ivar ends up holding.
class E
  attr_writer :w
  def set(v) = (self.w = v)
  def w = @w
end
e = E.new
p e.set(3)
p e.w

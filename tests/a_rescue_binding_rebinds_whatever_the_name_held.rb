# `rescue => e` assigns `e`, and ruby does not care what `e` was before. The
# name simply holds the exception inside the handler, and keeps holding it
# afterwards -- a rescue binding is an ordinary local assignment, not a scoped
# one.
#
# A compiler that infers a type per local has to widen the slot at that
# assignment. Keeping the type the name had BEFORE emitted the handler's
# `e.message` as a call on that earlier type while the value was an exception.

class Box
  def initialize(v)
    @v = v
  end

  def message = "box(#{@v})"
end

# A user-class local, then the same name as a rescue binding.
e = Box.new(1)
p e.message

begin
  raise ArgumentError, "boom"
rescue ArgumentError => e
  p e.message
  p e.class
end

# ... and it is STILL the exception after the begin.
p e.class
p e.message

# The other order: bound by a rescue first, then assigned an ordinary object.
begin
  raise "first"
rescue => x
  p x.message
end

x = Box.new(2)
p x.message

# An Integer local, and a String one -- the same widening, no user class in it.
n = 42
begin
  Integer("nope")
rescue ArgumentError => n
  p n.class
end
p n.is_a?(Exception)

s = "plain"
p s.upcase
begin
  raise TypeError, "typed"
rescue TypeError => s
  p s.message
end
p s.class

# One name across two clauses of the same begin, each raising a different class.
def attempt(which)
  raise ArgumentError, "arg" if which == :arg
  raise TypeError, "type"
end

[:arg, :type].each do |which|
  outcome = Box.new(which)
  begin
    attempt(which)
  rescue ArgumentError => outcome
    p [:arg, outcome.message]
  rescue TypeError => outcome
    p [:type, outcome.message]
  end
  p outcome.class
end

# A nested begin reusing the outer handler's name.
begin
  raise "outer"
rescue => err
  p err.message
  begin
    raise KeyError, "inner"
  rescue KeyError => err
    p err.message
  end
  p err.message
end

# The binding survives a handler that never ran, too: `rescue` did not fire, so
# the name keeps what it already had.
untouched = Box.new(:kept)
begin
  :fine
rescue => untouched
  p :never
end
p untouched.message

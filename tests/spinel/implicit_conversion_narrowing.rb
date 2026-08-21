# The conversion protocol reached through a NARROWING rather than an argument.
# A local written from a poly source whose every read is an int context, one of
# them an array index, is typed Integer (analyze.c: the index-narrowing pass) --
# and the poly value is unboxed at the write. That unboxing read a user object
# as 0, so the index silently named the wrong element: the narrowing pass reads
# an array index as PROOF the value is an integer, where CRuby reads it as the
# demand that it become one, or raise.
class Idx
  def to_int
    1
  end
end

class Inert
end

# 1. converts through #to_int, so the index is 1 (was element 0)
b = [10, Idx.new][1]
p [10, 20, 30][b]

# 2. the same shape through a named array
arr = [10, Idx.new]
c = arr[1]
p [10, 20, 30][c]

# 3. no conversion method: CRuby's TypeError (was a silent element 0). This
#    program defines no #to_int anywhere, which is the case a per-program gate
#    would have had to leave behind.
begin
  d = [10, Inert.new][1]
  p [10, 20, 30][d]
rescue TypeError => e
  p [e.class, e.message]
end

# An EXPLICIT #to_i is the method, named by the program, not a slot demanding an
# Integer: an object without it is NoMethodError, and one with it answers.
class HasToI
  def to_i
    42
  end
end
begin
  p [1, Inert.new][1].to_i
rescue NoMethodError => e
  p [e.class, e.message]
end
p [1, HasToI.new][1].to_i

# The int slots the runtime fills SPECULATIVELY must still accept an object:
# each of these publishes its value boxed as well, and the unboxed copy is read
# only by a callee that wants a number. Raising there broke curry, Pathname#join
# and CSV headers.
show = ->(label, value) { "#{label}=#{value.class}" }
p show.call("obj", Inert.new)                  # proc argument slot
p [Inert.new, Inert.new].reduce { |a, _b| a }.class
def pick(a, b) = a
f = method(:pick).to_proc.curry
p f[Inert.new][2].class                        # curry argument slots

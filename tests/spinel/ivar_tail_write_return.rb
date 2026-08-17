# A plain tail instance-variable write is the method's value in Ruby
# (`def m; @x = v; end` answers v). Returning the statement default instead
# handed back the slot type's nil: NULL for an Array (the caller segfaulted on
# the first `.length`), 0 for an Integer, "" for a String, {} for a Hash.
# #3317 routed only an OBJECT-valued tail ivar write through the value path,
# and `def m = (@x = v)` parses as a value so the endless form was already
# right; the ordinary statement form was not. (matz/spinel#1484 covers the
# operator-assignment form, which already worked.)
class Slots
  def initialize
    @i = nil
    @s = nil
    @a = nil
    @h = nil
    @f = nil
  end

  def int_tail;   @i = 7;         end
  def str_tail;   @s = "hi";      end
  def arr_tail;   @a = [1, 2, 3]; end
  def hash_tail;  @h = { a: 1 };  end
  def float_tail; @f = 2.5;       end
end

s = Slots.new
p s.int_tail
p s.str_tail
p s.arr_tail
p s.hash_tail
p s.float_tail

# The value is returned AND stored: the slot keeps it for the next read.
class Once
  def initialize; @v = nil; end
  def store; @v = [4, 5]; end
  def read; @v; end
end
o = Once.new
p o.store
p o.read

# The classic memoization guard: first call must build and answer the value,
# not the unassigned slot. The Array case used to segfault the caller here.
class Memo
  def initialize; @cache = nil; end
  def build
    return @cache if @cache
    @cache = [1, 2, 3]
  end
end
m = Memo.new
puts m.build.length
puts m.build.length

# Assigning a value computed into a local, with the write as the last
# expression (no trailing bare read to rescue it).
class FromLocal
  def initialize; @c = nil; end
  def build
    r = []
    3.times { |i| r << i }
    @c = r
  end
end
p FromLocal.new.build

# A tail write whose value came in as a parameter.
class FromParam
  def initialize; @p = nil; end
  def set(list); @p = list; end
end
p FromParam.new.set([9, 9])

# The endless-def form keeps working (it already did).
class Endless
  def initialize; @e = nil; end
  def set = (@e = [1, 2, 3])
end
p Endless.new.set

# The write is evaluated exactly once -- a side-effecting value expression
# must not run twice now that the slot is read back.
class OnceOnly
  def initialize; @n = nil; @calls = 0; end
  def bump; @calls += 1; [@calls]; end
  def set; @n = bump; end
  def calls; @calls; end
end
oo = OnceOnly.new
p oo.set
p oo.calls

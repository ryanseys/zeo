# A tail instance-variable write is the method's value: `def m; @x = v; end`
# answers v. That holds for every type the slot can be -- Array, Integer,
# String, Hash -- and for the ordinary statement form as much as the endless
# `def m = (@x = v)`, which parses as a value already.
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
__END__
7
"hi"
[1, 2, 3]
{a: 1}
2.5
[4, 5]
[4, 5]
3
3
[0, 1, 2]
[9, 9]
[1, 2, 3]
[1]
1

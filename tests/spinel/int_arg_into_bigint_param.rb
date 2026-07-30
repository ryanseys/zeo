# A parameter that sees both a small Integer and a Bignum types as bigint --
# ty_unify keeps them together rather than widening to poly, on the premise
# that an int is promoted at the boundary.
#
# The promotion was only ever wired into the arithmetic operands, so a
# parameter that reached bigint any other way got an mrb_int handed to an
# sp_Bigint* slot: ill-typed C, caught only by -Werror.

def any(x)
  x.to_s
end
p any(1)
p any(2**200)

def arith(n)
  n * 2 + 1
end
p arith(3)
p arith(2**70)

def cmp(a, b)
  a > b
end
p cmp(5, 2**80)
p cmp(2**80, 5)

# the default-value path binds the same slot
def opt(x = 7)
  x.to_s
end
p opt
p opt(2**100)

# keyword and splat arguments reach it too
def kw(x:, y: 3)
  (x + y).to_s
end
p kw(x: 1)
p kw(x: 2**90, y: 2)

def rest(*xs)
  xs.map { |v| v.to_s }
end
p rest(1, 2**70)

# a constructor argument is the same binding
class Box
  def initialize(v)
    @v = v.to_s
  end

  def show
    @v
  end
end
p Box.new(9).show
p Box.new(2**90).show

# The same boundary on the SLOT side: an ivar or class variable that elsewhere
# holds a Bignum takes an int write through the same promotion. Without it
# `@n = 0` wrote the integer 0 into an sp_Bigint * slot -- a null pointer
# constant, so the C compiled clean and the first sp_bigint_add segfaulted.
class Acc
  attr_reader :n

  def initialize(seed = 0)
    @n = seed
  end

  def add(v)
    @n = @n + v
    @n
  end

  def bump
    @n += 1     # no C operator exists for a bigint slot
    @n
  end

  def reset
    @n = 0
  end
end

a = Acc.new
p a.add(1)
p a.add(2**80)
p a.bump
p a.n
p a.reset
p a.add(2**64)
p Acc.new(5).add(2**70)

class Tally
  @@total = 0

  def self.bump(x)
    @@total = @@total + x
    @@total
  end
end
p Tally.bump(1)
p Tally.bump(2**80)

# a conditional whose arms are an int and a Bignum
class Either
  def initialize(f)
    @v = f ? 0 : 2**90
  end

  def show = @v.to_s
end
p Either.new(true).show
p Either.new(false).show

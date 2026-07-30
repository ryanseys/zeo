# A yielding method reached through a poly receiver.
#
# Blocks are fused, not passed: with a known receiver the method body is
# inlined at the call site with the block spliced into the yield. That leaves
# no symbol to call, so a poly dispatch dropped the arm and raised
# NoMethodError naming a class that plainly defines the method -- and with it
# went the whole block API of any class reached through a union.
#
# Such a method now also gets a second, ordinary definition taking the block as
# an sp_Proc *, with `yield` lowered to a call on it. Monomorphic call sites
# keep the splice; only the poly path pays for the closure.
#
# The clone is typed independently of the inlined original: its yield answers
# poly, so everything the yield feeds widens with it and one body serves every
# call site. That is what lets a yield nested inside a block work, and what
# lets two sites pass blocks of differing result type.

class A
  def go
    yield 5
  end

  def guarded
    block_given? ? yield(1) : 99
  end

  def early
    yield 1
    return 42
  end

  def withargs(a, b)
    yield a + b
  end

  def twice
    yield(1) + yield(2)
  end
end

class B
  def go
    0
  end

  def guarded
    -1
  end

  def early
    -3
  end

  def withargs(a, b)
    a - b
  end

  def twice
    -4
  end
end

def pick(n)
  n == 1 ? A.new : B.new
end

p pick(1).go { |v| v % 2 }
p pick(2).go { |v| v % 2 }

p pick(1).guarded { |v| v + 100 }
p pick(1).guarded
p pick(2).guarded

p pick(1).early { |v| v }
p pick(2).early { |v| v }

p pick(1).withargs(3, 4) { |s| s * 10 }
p pick(2).withargs(3, 4) { |s| s * 10 }

p pick(1).twice { |v| v * 3 }
p pick(2).twice { |v| v * 3 }

# a monomorphic call to the same method still fuses
a = A.new
p a.go { |v| v + 1 }
p a.twice { |v| v }

# a yield nested inside a block of the method's own body: the clone's locals
# widen with the yield, so `t` is a boxed carrier rather than an mrb_int
class Looper
  def looped
    t = 0
    3.times { |i| t += yield(i) }
    t
  end
end

class NoLoop
  def looped
    -2
  end
end

def pick_l(n)
  n == 1 ? Looper.new : NoLoop.new
end

p pick_l(1).looped { |i| i * 2 }
p pick_l(2).looped { |i| i * 2 }

# two poly call sites whose blocks answer different types: one clone serves
# both, because its yield is poly and each site unboxes into its own slot
p pick(1).go { |v| v % 2 }
p pick(1).go { |v| "s#{v}" }
p pick(2).go { |v| "s#{v}" }

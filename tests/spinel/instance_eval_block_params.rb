# instance_eval block-param shapes: self-bound first arg, extras
# bind to nil. Lifted sp_ieval_<N> still takes only `self`.

# ---- 1. Explicit-empty `{ || ... }` ----
class Box
  def initialize
    @v = 0
  end

  def v
    @v
  end
end

b1 = Box.new
b1.instance_eval { || @v = 42 }
puts b1.v               # 42

b1.instance_eval { @v = 7 }
puts b1.v               # 7

# ---- 2. Single self-bound `{ |x| ... }` (read) ----
class Counter
  def initialize
    @n = 0
  end

  def add(k)
    @n = @n + k
  end

  def n
    @n
  end
end

c = Counter.new
c.instance_eval { |me| me.add(5) }
puts c.n                # 5

c.instance_eval { |obj| obj.add(3); obj.add(7) }
puts c.n                # 15

# ---- 3. Extras default nil `{ |x, y, z| ... }` ----
class Foo
  def value
    42
  end
end

f = Foo.new
puts f.instance_eval { |a| a.value }              # 42
puts f.instance_eval { |a, b| a.value }           # 42
puts f.instance_eval { |a, b, c| a.value }        # 42

# ---- 4. Self-bound write `{ |x| x = ...; x.method }` ----
# Reassigning x rebinds the local, not the block's self -- a
# keeps only the first add.
a = Counter.new
a.instance_eval { |x|
  x.add(1)
  x = Counter.new
  x.add(10)
  x.add(20)
}
puts a.n                # 1

# ---- 5. Numbered param `{ _1.method }` ----
class Tally
  def initialize
    @x = 0
  end

  def bump
    @x = @x + 1
  end

  def add(k)
    @x = @x + k
  end

  def x
    @x
  end
end

t = Tally.new
t.instance_eval { _1.bump }
t.instance_eval { _1.bump }
t.instance_eval { _1.add(10) }
puts t.x                # 12

puts "done"

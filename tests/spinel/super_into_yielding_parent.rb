# `super` reaching a parent that yields. A yielding method has no standalone
# function -- it is inlined at every call site -- so the child has to be
# inlined too, carrying its caller's block down to the parent. Before this the
# call was left referencing a function nobody emitted and the link failed.

class Base
  def go(x)
    p ["base", x]
    yield(x * 2)
  end

  def each_thing
    yield 1
    yield 2
    :done
  end

  def self.build(n)
    p ["cbuild", n]
    yield n
  end
end

class Mid < Base
  def go(x)
    p ["mid", x]
    super
  end

  def each_thing
    p "mid-each"
    super
  end

  def self.build(n)
    p ["cmid", n]
    super
  end
end

class Leaf < Mid
  def go(x)
    p ["leaf", x]
    super
  end
end

# a three-link chain
p Leaf.new.go(3) { |v| v + 1 }
# the middle link called directly, with two differently-typed blocks
p Mid.new.go(5) { |v| v + 2 }
p Mid.new.go(7) { |v| "got #{v}" }
# several yields under the super
acc = []
p Mid.new.each_thing { |v| acc << v }
p acc
# a class-method super
p Mid.build(4) { |v| v * 10 }

# `super(args)`: the parent sees the rewritten argument, not the original
class B2
  def run(a)
    yield a
  end
end
class S2 < B2
  def run(a)
    super(a + 1)
  end
end
p S2.new.run(1) { |v| v * 100 }

# a splat parameter travels down the same path
class B3
  def all(*xs)
    p ["b3", xs]
    yield xs.length
  end
end
class S3 < B3
  def all(*xs)
    p ["s3", xs]
    super
  end
end
p S3.new.all(1, 2, 3) { |n| n * 5 }

# a parent that does NOT yield still goes through a plain call
class B4
  def plain(a) = a * 2
end
class S4 < B4
  def plain(a) = super + 1
end
p S4.new.plain(5)

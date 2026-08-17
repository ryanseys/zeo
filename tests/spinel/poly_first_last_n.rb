# `first(n)` / `last(n)` on a value whose type is only known at run time, in a
# program where a user class owns the name (which is what routes the call
# through the dispatch at all).
class Bag
  def initialize(a)
    @a = a
  end

  def first(n = 1)
    @a[0, n]
  end

  def last(n = 1)
    @a[-n, n]
  end
end

def pick(f)
  f ? [1, 2, 3, 4] : nil
end

x = pick(true)
p x.first
p x.first(2)
p x.last
p x.last(2)
p x.first(0)
p x.first(99)

b = Bag.new([7, 8, 9])
p b.first
p b.first(2)
p b.last(2)

def pickh(f)
  f ? { "a" => 1, "b" => 2 } : nil
end
p pickh(true).first(1)

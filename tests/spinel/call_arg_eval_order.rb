$n = 0

def nxt
  $n += 1
  $n
end

def three(a, b, c)
  [a, b, c]
end

class Box
  def initialize(a, b, c, d)
    @a = a
    @b = b
    @c = c
    @d = d
  end

  def to_a
    [@a, @b, @c, @d]
  end
end

p three(nxt, nxt, nxt)
p Box.new(nxt, nxt, "%d!" % [nxt], nxt).to_a
p three(nxt, [nxt, nxt], nxt)

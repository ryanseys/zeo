# Several methods share the forwarded name; the one that KEEPS the block
# decides whether the forwarder's captures need cells.
class Decoy
  def store(&b)
    b.call
  end
end

class Holder
  def store(&b)
    @proc = b
  end

  def run
    @proc.call
  end
end

def forward(h, &b)
  h.store(&b)
end

h = Holder.new
n = 0
forward(h) { n += 1 }
3.times { h.run }
p n

# the decoy still works on its own
d = Decoy.new
m = 0
forward(d) { m += 10 }
p m

# and the order the classes are defined in must not matter
class Later
  def keep(&b)
    @b = b
  end

  def fire
    @b.call
  end
end

def fwd2(o, &b)
  o.keep(&b)
end

k = 0
l = Later.new
fwd2(l) { k += 2 }
2.times { l.fire }
p k

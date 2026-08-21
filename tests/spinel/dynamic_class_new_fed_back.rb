# `s.class.new(**h)` on a receiver known only at run time. The dispatch
# switches on the class's cls_id, and the runtime helper that produces that
# class answered -1 for every value -- it filled in the NAME only -- so the
# switch matched no arm and the call returned nil (#4020). A statically typed
# receiver takes a different path, which is why it worked there.
S = Struct.new(:o, :b, :st, keyword_init: true)

def bump(s) = s.class.new(**s.to_h.merge(b: s.b + 1))
def rep(es, from) = es.reduce(from) { |st, _e| bump(st) }

x = S.new(o: "a", b: 0, st: :open)
p rep([1], x).to_h

# feeding the result back is what the report needed; each step alone works too
y = bump(x)
p y.class
p y.b
p y.to_h
p bump(y).to_h
p bump(bump(x)).to_h

# a reassigning loop is the same shape as the reduce
z = x
2.times { z = bump(z) }
p z.to_h

# the statically typed receiver keeps working
p x.class.new(**x.to_h.merge(b: 9)).to_h

# and a plain class, not just a Struct
class P
  attr_reader :n
  def initialize(n)
    @n = n
  end
end

def grow(p) = p.class.new(p.n + 1)
q = grow(P.new(1))
p q.n
p grow(q).n

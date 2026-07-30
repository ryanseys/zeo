# `h[k] op= v` used as an expression, with a receiver or key that is not a
# plain variable or literal.
#
# The value form ran the write and then re-read the slot to produce the value,
# which re-emitted the receiver and key expressions -- so it accepted only
# shapes it could safely evaluate twice, and declined everything else outright.
# A method-call key is the common one: `v[s.tag] += 1` as the last expression
# of a block is in value position, and a block lifted to a proc puts it there.
#
# Both are evaluated once now, into temps the write and the read-back share,
# which is also what CRuby promises.

# the reported shape: an indexed operator-write as a block's value, on a
# captured local, keyed by a method call
class Tagged
  def initialize(t)
    @t = t
  end

  def tag
    @t
  end
end

v = {}
[Tagged.new("a"), Tagged.new("a"), Tagged.new("b")].each do |s|
  v[s.tag] ||= 0
  v[s.tag] += 1
end
p v

# in value position directly
w = {}
w["k"] ||= 0
p(w["k"] += 5)
p w

# the key is evaluated exactly once
$calls = 0
def key_once
  $calls += 1
  "k"
end

h = {"k" => 10}
p(h[key_once] += 1)
p $calls
p h

# the receiver is evaluated exactly once
$rcalls = 0
def recv_once(x)
  $rcalls += 1
  x
end

g = {"k" => 1}
p(recv_once(g)["k"] += 2)
p $rcalls
p g

# an array receiver, with a computed index
a = [1, 2, 3]
def idx
  1
end
p(a[idx] += 10)
p a

# a nested receiver expression
m = {"outer" => [5]}
p(m["outer"][0] += 1)
p m

# string values fold too
s = {"k" => "x"}
p(s[key_once] += "y")
p s

# a poly-keyed hash reached through a method call on the key
class Box
  def initialize(n)
    @n = n
  end

  def n
    @n
  end
end

counts = {}
[Box.new(1), Box.new(1), Box.new(2)].each do |b|
  counts[b.n] ||= 0
  counts[b.n] += 1
end
p counts

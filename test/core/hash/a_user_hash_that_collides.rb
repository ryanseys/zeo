# A user `hash` is a BUCKET, not an identity. Ruby does not require it to
# be unique -- two distinct keys may collide -- so a table that keys on it
# alone loses entries and reads back the wrong value.
#
# This one did: two colliding keys stored as ONE, `h[a]` answered `"b"`,
# `uniq` collapsed them, and a `Set` held one member. Silent, with a clean
# exit, on the conventional way to write a value object.

class K
  attr_reader :n

  def initialize(n) = @n = n
  def hash = 42
  def eql?(o) = o.is_a?(K) && o.n == @n
  def ==(o) = eql?(o)
end

a = K.new(1)
b = K.new(2)
a2 = K.new(1)

h = {}
h[a] = "a"
h[b] = "b"
p h.size
p h[a]
p h[b]
# A DIFFERENT object that is `eql?` to a stored key still finds it -- the
# other half of the rule, and the reason identity alone is not the answer.
p h[a2]
p h.key?(b)
h[a2] = "a again"
p h.size
p h[a]

h.delete(a)
p h.size
p h[b]

p [a, b, a2].uniq.size

require "set"
s = Set.new([a, b, a2])
p s.size
p s.include?(b)

# A class defining `hash` but NO `eql?` keeps ruby's default identity, so
# two colliding instances stay two keys.
class OnlyHash
  def hash = 7
end
g = {}
g[OnlyHash.new] = 1
g[OnlyHash.new] = 2
p g.size

# `compare_by_identity` ignores both methods.
i = {}.compare_by_identity
i[a] = "a"
i[a2] = "a2"
p i.size
__END__
2
"a"
"b"
"a"
true
2
"a again"
1
"b"
2
2
true
2
2

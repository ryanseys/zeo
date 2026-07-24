# Array#[]= in its SPLICE forms -- `a[start, len] = v` and `a[range] = v` --
# replaces the span with the value's elements (CRuby's rb_ary_splice), which
# is a different operation from the one-index store `a[i] = v`.

# The plain index store still just stores.
a = [1, 2, 3]
a[1] = 9
p a
a[5] = 7
p a
a[-1] = 0
p a

# start/length: the span is replaced by the VALUE's elements.
b = [1, 2, 3, 4]
b[1, 2] = [:x, :y, :z]
p b

# Replacing with fewer elements shrinks the array.
c = [1, 2, 3, 4]
c[1, 3] = [:only]
p c

# A zero length INSERTS without removing.
d = [1, 2, 3]
d[1, 0] = [:a, :b]
p d

# A length past the end just truncates the span.
e = [1, 2, 3]
e[1, 99] = [:end]
p e

# Splicing at the end appends; past the end nil-pads first.
f = [1, 2]
f[2, 0] = [:x]
p f
g = [1]
g[3, 0] = [:y]
p g

# A non-Array value inserts as ONE element.
h = [1, 2, 3]
h[0, 2] = :single
p h

# Negative starts count from the end.
i = [1, 2, 3, 4]
i[-2, 2] = [:last]
p i

# Range forms: inclusive and exclusive.
j = [1, 2, 3, 4]
j[1..2] = [:a]
p j
k = [1, 2, 3, 4]
k[1...3] = [:b]
p k

# An endless / beginless range.
l = [1, 2, 3, 4]
l[2..] = [:tail]
p l
m = [1, 2, 3, 4]
m[..1] = [:head]
p m

# A range with a negative bound.
n = [1, 2, 3, 4]
n[-2..] = [:z]
p n

# The expression VALUE is the right-hand side as written, never the coercion
# or the receiver.
o = [1, 2, 3]
r = (o[0, 1] = [:v])
p r

# A user object defining to_ary is coerced through it, and its elements are
# spliced -- while the expression value stays the object itself.
class IntPair
  def initialize(a, b)
    @a = a
    @b = b
  end

  def to_ary
    [@a, @b]
  end
end

q = [1, 2, 3]
q[1, 1] = IntPair.new(7, 8)
p q

s = [1, 2, 3, 4]
s[1..2] = IntPair.new(9, 9)
p s

t = [1, 2, 3]
value = (t[1, 1] = IntPair.new(6, 6))
p value.is_a?(IntPair)
p t

# An object without to_ary inserts as a single element.
class Opaque; end
u = [1, 2, 3]
u[0, 2] = Opaque.new
p u.length
p u[1]

# A negative length raises.
begin
  [1, 2, 3][0, -1] = [:x]
rescue IndexError => e
  puts "IndexError: #{e.message}"
end

# A too-small negative index raises.
begin
  [1, 2, 3][-9, 1] = [:x]
rescue IndexError => e
  puts "IndexError: #{e.message}"
end

# Splicing works through a dynamically-typed receiver too.
nested = [[1, 2, 3], "x"]
nested[0][1, 1] = IntPair.new(4, 5)
p nested[0]

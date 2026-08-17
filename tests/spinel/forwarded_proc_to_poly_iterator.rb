# `xs.each(&proc)` on a receiver whose type is only known at run time drove an
# empty loop body: the appends simply vanished.
def poly(n)
  n > 0 ? [5, 6] : nil
end

out = []
k = proc { |v| out << v }
poly(1).each(&k)
p out

def run(xs)
  acc = []
  xs.each do |row|
    h = proc { |v| acc << v }
    row.each(&h)
  end
  acc
end

p run([[1], [2, 3]])

# the typed receivers keep working
t = []
g = proc { |v| t << v * 2 }
[1, 2].each(&g)
p t

# each_with_index through the same path
seen = []
w = proc { |v, i| seen << "#{i}:#{v}" }
poly(1).each_with_index(&w)
p seen

# value-returning forms take the same driver
def poly2(n)
  n > 0 ? [1, 2, 3] : nil
end

dbl = proc { |v| v * 10 }
p poly2(1).map(&dbl)
odd = proc { |v| v.odd? }
p poly2(1).select(&odd)
p poly2(1).reject(&odd)
p poly2(1).map(&:to_s)
p poly2(1).find(&odd)
p poly2(1).count(&odd)

# a Hash receiver still answers its own face
def polyh(n)
  n > 0 ? { "a" => 1, "b" => 2 } : nil
end
pair = proc { |k, v| "#{k}=#{v}" }
p polyh(1).map(&pair)

# non-integer elements through the same driver
def polys(n)
  n > 0 ? ["a", "b"] : nil
end
up = proc { |v| v.upcase }
p polys(1).map(&up)
seen2 = []
coll = proc { |v| seen2 << v }
polys(1).each(&coll)
p seen2

# A wide, shallow call fan-out inside a fiber: every arm forced inline is
# absorbed into one frame as a sibling of the others, and the rooted locals
# cannot share slots, so the frame grows with the BREADTH of the router. The
# full reproducer (350 arms) overran the 64K fiber stack; this keeps the shape
# small enough to be a behaviour check.
def leaf_a(s)
  x = s.upcase
  y = s.downcase
  x.length + y.length
end

def leaf_b(s)
  x = s + "!"
  y = x * 2
  y.length
end

def leaf_c(s)
  x = [s, s]
  x.map { |v| v.length }.sum
end

def router(k, s)
  r = 0
  if k == 0
    r = leaf_a(s)
  elsif k == 1
    r = leaf_b(s)
  elsif k == 2
    r = leaf_c(s)
  else
    r = leaf_a(s) + leaf_b(s) + leaf_c(s)
  end
  r
end

f = Fiber.new do
  n = ARGV.length
  puts router(n, "abc")
  puts router(n + 1, "abc")
  puts router(n + 2, "abc")
  puts router(n + 9, "abc")
end
f.resume

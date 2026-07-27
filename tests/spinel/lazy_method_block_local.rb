# A local assigned inside a .lazy block whose chain is spliced from a method
# body has to be declared where the chain lands, not where it was written.
def f
  ["a 5"].lazy.map { |l| q1 = l.split(" "); q1[0] }
end
p f.to_a

def g
  [1, 2, 3].lazy.map { |n| d = n * 2; d + 1 }
end
p g.to_a
p g.first

def h
  ["x,y"].lazy.map { |s| parts = s.split(","); parts.last }
end
p h.first

# the same chain inline still works
p ["a 5"].lazy.map { |l| q = l.split(" "); q[0] }.to_a

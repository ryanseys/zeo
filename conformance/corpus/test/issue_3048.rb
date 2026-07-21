pr = proc { |x| (x || 0) + 1 }
d = pr.dup
p d.equal?(pr)
p d.call(41)
c = pr.clone
p c.equal?(pr)
p c.call(1)
n = 0
inc = proc { n += 1 }
inc.dup.call
inc.call
p n
lam = lambda { |a| a * 2 }
p lam.dup.lambda?
p lam.dup.call(21)

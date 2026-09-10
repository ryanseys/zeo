# `&(blk)`, `&(proc { })` and `&(lam)` each pass the proc as the block.
blk = proc { |x| x * 2 }
p [1, 2].map(&(blk))
p [1, 2, 3].select(&(proc { |x| x.odd? }))
lam = ->(x) { x + 1 }
p [1, 2].map(&(lam))
p [3, 1, 2].sort_by(&(proc { |x| -x }))
__END__
[2, 4]
[1, 3]
[2, 3]
[3, 2, 1]

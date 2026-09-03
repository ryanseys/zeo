# A nested escaping closure that captures the ENCLOSING block's own local
# (not just the block param). That local must become a shared cell, declared
# once -- previously the own-locals prelude also fresh-declared it, shadowing
# the cell (the inner closure read nil), or tripped an internal invariant.

adders = []
[1, 2, 3].each do |n|
  base = n * 100          # the block's own local
  adders << -> { base + n }
end
p adders.map(&:call)      # [101, 202, 303]

# The own local is mutated after the capturing closures are built, and every
# closure observes the final value through the shared cell.
procs = []
[10, 20].each do |k|
  acc = 0
  procs << -> { acc }
  acc = k + 1
  procs << -> { acc }
end
p procs.map(&:call)       # [11, 11, 21, 21]
__END__
[101, 202, 303]
[11, 11, 21, 21]

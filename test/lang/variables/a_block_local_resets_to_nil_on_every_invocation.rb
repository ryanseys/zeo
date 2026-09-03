# The part that makes a block-local more than a naming convention, and
# the reason it can't be hoisted out of the per-call prologue: `total`
# is nil again at the top of EVERY call, so this never accumulates.

total = 42
[1, 2, 3].each { |x; total| total = (total || 0) + x }
p total

outs = []
[1, 2].each { |x; acc| acc ||= []; acc << x; outs << acc.dup }
p outs
__END__
42
[[1], [2]]

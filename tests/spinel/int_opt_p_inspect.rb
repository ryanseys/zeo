# `p` and inspect on a nullable int (int?) print "nil" for the
# missing/zero-sentinel case, not blank or the raw sentinel.
p 0.nonzero?
p 42.nonzero?
p [].first
x = 0.nonzero?
p x
puts 0.nonzero?.inspect

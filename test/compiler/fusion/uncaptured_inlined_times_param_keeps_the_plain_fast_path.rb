# Regression guard for H1: when NOTHING captures the `.times` param it
# stays a plain per-iteration `let` (no cell), and `break`/`next` inside
# the inlined block keep working via literal labels.

total = 0
5.times { |i| total += i }
puts total
r = 5.times { |i| break i * 2 if i == 3 }
p r
__END__
10
6

# A multi-assign target is a plain outer assignment for Ruby's textual
# block-local rule, positioned at its own statement. Before: `level` here
# counted its RE-assignment inside the loop as the scope's first, demoting
# it to loop-block-local and refusing the nested `find` (minitest's
# find_minimal_combination). After a block, the same shape stays untouched
# by the block's own write -- both directions oracle-verified.
def f
  level, n = 1, 1
  loop do
    r = [1, 2, 3].find { |a| a > level }
    level += 1
    break r if level > 2
  end + n
end
p f

pr = proc { x = 99 }
x, y = 5, 6
pr.call
p x
p y
__END__
4
5
6

# `zip`'s arguments must respond to :each. The emitters read every argument as
# a container regardless of its type, so a nil quietly became a column of nils
# and an Integer or a String stopped the C build -- found verifying the gaps
# left after PR #4029, whose sweep of the typed slots did not reach this one.
#
# A Hash and an Enumerator DO respond to :each, and were the same ill-typed C
# for the opposite reason: the slot was spelled sp_PolyArray* and handed an
# sp_SymPolyHash*.
def try
  yield
rescue => e
  p [e.class, e.message]
end
# valid forms keep working
p [1,2].zip([3,4])
p [1,2].zip([3,4], [5,6])
p [1,2].zip([3])
p [1,2].zip(1..2)
p ["a","b"].zip([1,2])
p [1.5].zip([2.5])
p [1,2].zip({a: 1, b: 2})
p [1,2].zip([3,4].each)
p [[1],[2]].zip([[3],[4]])
def poly(f) = f ? [1,2] : "s"
p poly(true).zip([3,4])
xs = [[3,4], nil][0]
p [1,2].zip(xs)

# every scalar is CRuby's TypeError naming the class
try { [1,2].zip(nil) }
try { [1,2].zip([3,4], nil) }
try { [1,2].zip(5) }
try { [1,2].zip("ab") }
try { [1,2].zip(:s) }
try { [1,2].zip(true) }
try { [1,2].zip(1.5) }
try { ["a"].zip(nil) }
try { [1.5].zip(nil) }
try { poly(true).zip(nil) }

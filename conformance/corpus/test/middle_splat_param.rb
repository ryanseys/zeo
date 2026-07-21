# Issue #710. A middle *splat parameter (def f(a, *b, c)) must collect
# the inner args and leave the trailing fixed param(s) for the args
# after the splat. Before the fix, the splat greedily ate all the
# remaining call args and `c` got 0 with "missing required arg" warnings.

def g(a, *b, c); [a, b, c]; end
puts g(1, 2, 3, 4).inspect
puts g(10, 20, 30).inspect
puts g(100, 200).inspect          # empty splat, c=200

def h(a, b, *c, d, e)
  [a, b, c, d, e]
end
puts h(1, 2, 3, 4, 5, 6, 7).inspect

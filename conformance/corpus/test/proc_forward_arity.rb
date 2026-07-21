# Inline `&->(...)` lambdas and multi-param procs forwarded into iterators.
# Direct clone-param typing (an inline lambda's params are typed from the
# forwarded call site) plus arity-matched calling (a 1-param callable receives a
# hash pair as one array; a 2-param one receives k, v positionally) close the
# forwarding gaps. Distinct per-type helpers keep element types monomorphic.
def ints(a) = a
def strs(a) = a
def syms(a) = a
def hsh(x) = x
def shsh(x) = x

# inline lambdas now forward into arrays of any element type (previously the
# cloned lambda's param defaulted to int and mistyped non-int elements)
p ints([1, 2, 3]).map(&->(x) { x * 2 })                   # [2, 4, 6]
p strs(["a", "bb", "c"]).select(&->(s) { s.length == 1 }) # ["a", "c"]
p syms([:a, :bb]).map(&->(s) { s.to_s })                  # ["a", "bb"]

# inline lambda and 1-param value into Hash#each receive the [k, v] pair
h = hsh({ a: 1, b: 2 })
h.each(&->(pair) { p pair })                              # [:a, 1] / [:b, 2]
pair_l = ->(pair) { p pair[1] }
h.each(&pair_l)                                           # 1 / 2

# a 2-param proc receives k, v positionally (auto-splat by arity)
kv = proc { |k, v| p [k, v] }
h.each(&kv)                                               # [:a, 1] / [:b, 2]

# each_key / each_value forward the bare key / value
h.each_key(&->(k) { p k })                                # :a / :b
h.each_value(&->(v) { p v })                              # 1 / 2

# a string-keyed hash forwards string keys
shsh({ "x" => 10, "y" => 20 }).each_key(&->(k) { p k })   # "x" / "y"

# a proc/lambda called directly with a scalar arg gets that scalar's type,
# not a poly widening of the bare-int default
p ->(s) { s.upcase }.call("hi")                           # "HI"
p ->(s) { s.to_s }.call(:hello)                           # "hello"

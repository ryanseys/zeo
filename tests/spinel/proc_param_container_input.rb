# Call-site argument inference for proc/lambda params: a container argument
# (array/hash/string) overrides a param's bare-int default instead of widening
# to a poly scalar. This is what lets a 1-param proc/lambda VALUE forwarded into
# Hash#each receive the [k, v] pair (the pair array), and it also fixes a proc
# called directly with a container arg. Method objects take any hash yield via
# their array ABI. Distinct per-type helpers keep receiver types monomorphic.
def hsh(x) = x
def ints(x) = x

h = hsh({ a: 1, b: 2 })

# 1-param proc / lambda values receive the [k, v] pair
pair_l = ->(pair) { p pair }
h.each(&pair_l)                  # [:a, 1] / [:b, 2]
pair_p = proc { |pair| p pair.last }
h.each(&pair_p)                  # 1 / 2

# a Method object forwards each / each_key / each_value through its array ABI
def show(pair) = p(pair)
def showk(k) = p(k)
def showv(v) = p(v)
h.each(&method(:show))           # [:a, 1] / [:b, 2]
h.each_key(&method(:showk))      # :a / :b
h.each_value(&method(:showv))    # 1 / 2

# the underlying inference win: a proc called directly with a container arg gets
# the container type for its param, not a poly scalar
arr = ints([10, 20, 30])
total = ->(a) { a.sum }
p total.call(arr)                # 60
head2 = ->(a) { a.first(2) }
p head2.call(arr)                # [10, 20]

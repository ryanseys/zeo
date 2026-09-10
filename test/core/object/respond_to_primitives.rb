# respond_to? on built-in receivers, over the whole shape of the question: a
# no-arg method, an operator, an index, a block iterator, a String argument
# rather than a Symbol, and a name that is absent.
#
# Each receiver goes through its OWN helper so it keeps its concrete type. One
# shared helper would widen them all to a single slot, which is a different
# test.
def st(x); x; end
def si(x); x; end
def sf(x); x; end
def sa(x); x; end
def sh(x); x; end
def sy(x); x; end

# String: no-arg, operator, index, block iterator, String-form argument, absent
p st("hi").respond_to?(:upcase)      # true
p st("hi").respond_to?(:+)           # true
p st("hi").respond_to?(:each_char)   # true
p st("hi").respond_to?("downcase")   # true
p st("hi").respond_to?(:no_such)     # false
p st("hi").respond_to?(:push)        # false

# Integer: block-returning-self, operator, Comparable mixin, absent
p si(5).respond_to?(:times)          # true
p si(5).respond_to?(:+)              # true
p si(5).respond_to?(:between?)       # true
p si(5).respond_to?(:upcase)         # false

# Float
p sf(1.5).respond_to?(:ceil)         # true
p sf(1.5).respond_to?(:round)        # true

# Array: self-returning iterators, map, operator, absent
p sa([1, 2]).respond_to?(:each)             # true
p sa([1, 2]).respond_to?(:each_with_index)  # true
p sa([1, 2]).respond_to?(:map)              # true
p sa([1, 2]).respond_to?(:reverse_each)     # true
p sa([1, 2]).respond_to?(:upcase)           # false

# Hash: iterators + lookups
p sh({a: 1}).respond_to?(:each)      # true
p sh({a: 1}).respond_to?(:each_pair) # true
p sh({a: 1}).respond_to?(:key?)      # true
p sh({a: 1}).respond_to?(:fetch)     # true
p sh({a: 1}).respond_to?(:upcase)    # false

# Symbol
p sy(:foo).respond_to?(:upcase)      # true
p sy(:foo).respond_to?(:to_sym)      # true
p sy(:foo).respond_to?(:each)        # false

# Kernel/Object universals resolve for any receiver
p st("x").respond_to?(:class)        # true
p si(5).respond_to?(:frozen?)        # true
p sa([1]).respond_to?(:tap)          # true

# literal receivers (already concrete-typed)
p nil.respond_to?(:to_a)             # true
p nil.respond_to?(:upcase)           # false
p true.respond_to?(:&)               # true
p (1..3).respond_to?(:each)          # true
p (1..3).respond_to?(:map)           # true
__END__
true
true
true
true
false
false
true
true
true
false
true
true
true
true
true
true
false
true
true
true
true
false
true
true
false
true
true
true
true
false
true
true
true

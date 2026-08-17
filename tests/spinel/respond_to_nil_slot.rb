# respond_to? on a nil that arrived through a slot: the literal receiver
# folded at compile time, the slot did not, and answered a flat false (#3815).
a = nil
p a.respond_to?(:to_a)
p a.respond_to?(:to_i)
p a.respond_to?(:inspect)
p a.respond_to?(:definitely_not_a_method)
p nil.respond_to?(:to_a)
xs = [nil, 1]
p xs[0].respond_to?(:to_a)
b = true
p b.respond_to?(:&)
p b.respond_to?(:definitely_not)

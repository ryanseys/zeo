# Proc#parameters reflects the block/lambda signature. A proc reports required
# positionals as :opt (it doesn't enforce arity); a lambda reports them as :req.
p proc { |x| }.parameters
p lambda { |x| }.parameters

# Optional, splat, post, keyword, keyword-splat and block params, in order.
sig = proc { |a, b = 1, *rest, last, key:, opt: 2, **kw, &blk| }
p sig.parameters

# Lambda form of the same signature (required positionals become :req).
lam = ->(a, b = 1, *rest, last, key:, opt: 2, **kw, &blk) { }
p lam.parameters

# Anonymous splats report the sigil as their name.
p proc { |*| }.parameters
p proc { |**| }.parameters

# The Symbol#method_missing idiom: parameters travels with a stored proc.
doubler = ->(n) { n * 2 }
p doubler.parameters
p [1, 2, 3].map(&doubler)

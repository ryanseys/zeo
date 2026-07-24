# #449: `unless cond; return {hash}; end; nil` — the function's
# return type used to ignore the unless-arm's `return {hash}` and
# infer mrb_int (the trailing nil), failing C compile. Now the
# UnlessNode arm in collect_return_types mirrors IfNode, so both
# the unless-body's return and the trailing nil contribute to
# return type unification.

def self.outer(flag)
  unless flag
    return { x: "y" }
  end
  nil
end

puts outer(false).nil? ? "nil" : "found"
puts outer(true).nil? ? "nil" : "found"

# `unless cond` with else_clause — both arms can carry returns.
def self.classify(n)
  unless n.zero?
    return { kind: "nonzero" }
  else
    return { kind: "zero" }
  end
end

puts classify(0)[:kind]
puts classify(5)[:kind]

# Top-level def variant (non-self).
def first_or_default(arr, default = nil)
  unless arr.empty?
    return arr[0]
  end
  default
end

puts first_or_default(["hi", "bye"])
puts first_or_default([], "fallback")

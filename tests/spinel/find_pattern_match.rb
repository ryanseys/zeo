# Find patterns `case arr in [*head, m.., *tail]`: scan for the first window of
# requireds, binding the leading splat (head), required targets, and trailing
# splat (tail). Pre-fix the arm matched unconditionally with head/tail nil.

def classify(a)
  case a
  in [*head, :a, :b, *tail]
    "ab head=#{head.inspect} tail=#{tail.inspect}"
  in [*pre, x, :stop, *post]
    "stop x=#{x} pre=#{pre.inspect} post=#{post.inspect}"
  else
    "none"
  end
end

puts classify([:x, :y, :a, :b, :z])
puts classify([:a, :b])
puts classify([:p, :q, :stop, :r])
puts classify([:z])
puts classify([])

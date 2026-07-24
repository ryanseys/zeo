# A lambda with an early (non-local) return whose type diverges from the
# fall-through tail must publish the wider boxed return value.
f = ->(items) {
  items.each do |pr|
    out = pr.call
    return out if out
  end
  nil
}
p f.call([-> { "A" }])
p f.call([-> { false }])

g = ->(a) { a.each { |x| return x if x > 2 }; nil }
p g.call([1, 2, 3])
p g.call([1, 1])

# An each with `return out if out`, falling through to a different kind of
# value.
# (spinel issue #3241)
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
__END__
"A"
nil
3
nil

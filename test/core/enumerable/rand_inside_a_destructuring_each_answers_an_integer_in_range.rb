# `rand(odds)` inside `each { |odds, name| }` answers a non-negative Integer.
# (spinel issue #2897)
[[1000000, :a], [500000, :b]].each do |odds, name|
  r = rand(odds)
  p r.class
  p r >= 0
  p(r == 0 || r > 0)
end
__END__
Integer
true
true
Integer
true
true

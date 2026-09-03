# `n.times { }` on an integer literal is spliced inline as a native Rust
# loop rather than becoming a real Proc, so it binds its params at its
# own site and needs the block-local declaration applied there too.

n = "outer"
3.times { |i; n| n = i }
p n

f = ->(x; t) { t = x * 2; t }
p f.call(5)

q = "kept"
[[1, 2, 3]].each { |(x, y), *r; q| q = x }
p q
__END__
"outer"
10
"kept"

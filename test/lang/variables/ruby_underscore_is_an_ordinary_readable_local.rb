# Rust's `_` isn't a named binding at all (`let mut _` doesn't parse,
# and a macro `$x:ident` matcher rejects it), so it must be mangled.

_ = 10
p _
_ = _ + 5
p _
[[1, :a]].each { |n, _| p n }
[[1, :a]].each { |_, sym| p sym }
_, second = [:first, :second]
p [second, _]
__END__
10
15
1
:a
[:second, :first]

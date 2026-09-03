# `1e400` overflows to Infinity at parse time. It cannot be emitted as a
# Rust float TOKEN (`Literal::f64_suffixed` asserts `is_finite()` and
# panics inside proc-macro2), so codegen must emit the `f64` constant
# path instead -- the value is perfectly ordinary Ruby.

big = 1e400
p big
p(-1e400)
p big.infinite?
p (big - big).nan?
p [1e400, -1e400].max
__END__
Infinity
-Infinity
1
true
Infinity

# `Object.new` emitted `Arc::new(zeo_rt::Object)` -- using the struct
# `Object` (a private-field struct, not a unit struct) as a value, an
# E0423 that never compiled. It must be `Object::default()`. Each call is
# a fresh Arc, so two sentinels are distinct by identity.

A = Object.new
B = Object.new
p A == B
p A == A
p A.is_a?(Object)
__END__
false
true
true

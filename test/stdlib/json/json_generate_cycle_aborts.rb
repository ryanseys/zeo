# The cycle this program builds on purpose, and the census the
# `ZEO_RT_GCCHECK=1` leg gates against. A cycle alive at exit is not a
# defect: it is a ring the program never broke. What the leg gates is a
# CHANGE to the line below.
# 
# `a << a` -- the circular reference the generator is supposed to name.
#@ gccheck: cycle leak: 1 objects (Array x1)
# `JSON.generate` on a self-referential array raises JSON::NestingError
# ("Did you try to serialize objects with circular references?"); zeo's
# serializer re-enters the container guard and the process ABORTS
# (`value/collections.rs:174`) -- the third surface of the recursive-
# traversal guard family beside `Array#join` and twin-`<=>`. (Found by
# the 2026-08-24 probe sweep.)
require "json"
a = [1]
a << a
begin
  JSON.generate(a)
rescue JSON::NestingError => e
  puts e.class
end
__END__
JSON::NestingError

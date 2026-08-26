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

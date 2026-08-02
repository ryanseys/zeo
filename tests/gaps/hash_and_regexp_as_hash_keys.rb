# A Hash used as a Hash KEY is keyed by identity, not by value.
#
# `collections::HashKey` projects every key to a structural form -- String,
# Array, Range, Regexp, the whole numeric tower -- but has none for Hash, so
# a Hash key falls through to `Identity(pointer)`. Two equal literals are
# therefore two different keys, and `#hash` disagrees between them for the
# same reason. The REGEXP half of this file used to fail the same way and is
# fixed; it stays here as the control.
#
# What the Hash half needs beyond a variant: ruby's `Hash#hash` is order
# INSENSITIVE, and `HashKey` derives `PartialEq`, which over a `Vec` is order
# SENSITIVE. So a `Hash(Vec<(HashKey, HashKey)>)` variant would hash two
# equal-but-differently-ordered hashes alike and then compare them unequal --
# worse than today. Closing it means hand-writing `PartialEq` for the whole
# enum with a multiset comparison for this one variant, on the hottest
# equality surface in the collections layer. Worth doing deliberately, not
# as a rider.

def show(label)
  puts "#{label}: #{yield.inspect}"
rescue Exception => e # rubocop:disable Lint/RescueException
  puts "#{label}: #{e.class}: #{e.message}"
end

h = { { a: 1 } => "hash key", /re/ => "regexp key", [1, 2] => "array key" }
show("hash key") { h[{ a: 1 }] }
show("regexp key") { h[/re/] }
show("array key") { h[[1, 2]] }

show("Hash#hash agrees") { { a: 1 }.hash == { a: 1 }.hash }
show("Regexp#hash agrees") { /re/.hash == /re/.hash }
show("Array#hash agrees") { [1, 2].hash == [1, 2].hash }

# Order does not matter to `Hash#hash` in ruby.
show("Hash#hash ignores order") { { a: 1, b: 2 }.hash == { b: 2, a: 1 }.hash }

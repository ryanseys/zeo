# A Hash used as a Hash KEY was keyed by identity, not by value: two equal
# literals were two different keys, and `#hash` disagreed between them.
#
# `HashKey` projects every other key structurally, but a Hash fell through to
# `Identity(pointer)`. The obstacle was that ruby's `Hash#hash`/`#eql?` are
# order INSENSITIVE while a derived `PartialEq` over a `Vec` is order
# SENSITIVE -- so the pairs are put in a canonical order when the KEY is built
# instead of being compared as a multiset. That keeps the derived `PartialEq`
# correct and leaves the hottest equality surface in the collections layer
# untouched.
#
# The REGEXP half stays as the control; it used to fail the same way.

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

# Order does not matter to `Hash#hash` in ruby, and must not matter to lookup.
show("Hash#hash ignores order") { { a: 1, b: 2 }.hash == { b: 2, a: 1 }.hash }

g = { { a: 1 } => "one", { a: 1, b: 2 } => "two", {} => "empty" }
show("two keys") { g[{ a: 1, b: 2 }] }
show("reordered") { g[{ b: 2, a: 1 }] }
show("empty hash key") { g[{}] }
show("miss") { g[{ a: 2 }] }
show("size") { g.size }

# Nested, and with mixed key/value types inside the key.
n = { { a: { b: 1 } } => "nested" }
show("nested") { n[{ a: { b: 1 } }] }
show("nested miss") { n[{ a: { b: 2 } }] }
m = { { 1 => "a", :s => [1, 2], "str" => 1.5 } => "mixed" }
show("mixed") { m[{ "str" => 1.5, :s => [1, 2], 1 => "a" }] }

show("eql?") { { a: 1 }.eql?({ a: 1 }) }
show("uniq") { [{ a: 1 }, { a: 1 }, { b: 2 }].uniq.size }
require "set"
show("as a set member") { Set[{ a: 1 }, { a: 1 }].size }

# A key mutated after insertion is filed under a digest that no longer
# describes it, and `rehash` is what re-derives them. It was a no-op, on the
# belief that lookups digest afresh -- they do not, the map IS keyed by the
# digest. Broken for an Array key too, which is why both are checked.
k = { a: 1 }
hash_keyed = { k => "v" }
k[:b] = 2
show("stale hash key") { hash_keyed[{ a: 1, b: 2 }] }
show("rehashed hash key") { hash_keyed.rehash[{ a: 1, b: 2 }] }

arr = [1]
arr_keyed = { arr => "v" }
arr << 2
show("stale array key") { arr_keyed[[1, 2]] }
show("rehashed array key") { arr_keyed.rehash[[1, 2]] }
show("rehash returns self") { arr_keyed.rehash.equal?(arr_keyed) }
show("keys survive rehash") { arr_keyed.keys }
__END__
hash key: "hash key"
regexp key: "regexp key"
array key: "array key"
Hash#hash agrees: true
Regexp#hash agrees: true
Array#hash agrees: true
Hash#hash ignores order: true
two keys: "two"
reordered: "two"
empty hash key: "empty"
miss: nil
size: 3
nested: "nested"
nested miss: nil
mixed: "mixed"
eql?: true
uniq: 2
as a set member: 1
stale hash key: nil
rehashed hash key: "v"
stale array key: nil
rehashed array key: "v"
rehash returns self: true
keys survive rehash: [[1, 2]]

# A Hash or a Regexp used as a Hash KEY is keyed by identity, not by value.
#
# `collections::HashKey` projects every key to a structural form -- there are
# variants for String, Array, Range, the whole numeric tower -- but none for
# Hash or Regexp, so both fall through to `Identity(pointer)`. Two equal
# literals are therefore two different keys, and `#hash` disagrees between
# them for the same reason.
#
# Array keys work, which is what makes this a missing pair of variants rather
# than a missing mechanism. Fix shape: a `Regexp(source, flags)` variant
# (cheap and total), and a `Hash(Vec<(HashKey, HashKey)>)` one whose hash is
# order-INSENSITIVE, as CRuby's is -- taking care with a self-referential
# hash, which `#inspect` already has to handle.

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

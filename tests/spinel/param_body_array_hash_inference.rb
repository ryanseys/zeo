# #544 / #545 follow-up to #542. Body-usage param inference
# extended to two new shapes:
#
# A. Array body inference (#545 conservative path):
#    `param.push(x)` / `.pop` / `.shift` / `.unshift` / `.concat`
#    / `.compact` / `.flatten` / `.transpose` widens an
#    int/nil-defaulted param to poly_array. Critically does NOT
#    fire on `<<`, `&`, `|`, `*`, `+`, `-` (overlap with
#    Integer bitwise/arithmetic -- optcarrot's poke(data) shape).
#    Runs ONCE post-fixpoint so call-site inference can pin
#    narrower variants (int_array / float_array / etc.) first.
#
# B. Hash iteration inference (#545 iteration arm, lifted to
#    #542's pass):
#    `param.keys` / `.values` / `.each_pair` / `.merge` /
#    `.has_key?` / `.fetch` / `.store` / `.delete` /
#    `.transform_values` / `.transform_keys` / `.to_h` widens
#    to str_poly_hash when no literal-key signal has already
#    pinned the more-specific variant. These methods don't exist
#    on Array / String / Integer, so the classifier is
#    unambiguous.
#
# Updated to MRI-compat after #634 shape B landed. The prior
# fixture seeded the body widening via `b.contents` on an
# uninitialized `attr_accessor :contents` whose read returned
# the int placeholder (cast to NULL at the call site, with
# runtime NULL-guards no-op'ing the body). That setup contradicts
# MRI -- `nil.push` raises NoMethodError. The body-widening
# passes themselves still need test coverage; we keep that by
# seeding from concrete literal arguments instead.

# Array body inference - typed caller via an array literal.
def consume_arr(arr)
  arr.push(42)
  puts "arr.length=" + arr.length.to_s
end

consume_arr([])

# Hash iteration inference -- .each + .merge / .keys + [k].
def merge_into_seed(other)
  result = {a: 1}
  other.each { |k, v| result[k] = v }
  result.keys.length
end

puts merge_into_seed({b: 2, c: 3})

# Hash widening via .keys (no .each, no [k] literal access) --
# Sam's SqliteAdapter#insert/update shape (`cols = attrs.keys;
# cols.map { |k| attrs[k] }` -- the [k] uses a computed key,
# not a literal, so #542's literal-key arm misses but .keys
# catches the param's Hash shape).
def hash_keys_count(h)
  h.keys.length
end

puts hash_keys_count({a: 1, b: 2})

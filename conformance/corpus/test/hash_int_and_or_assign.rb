# h[k] &&= and h[k] ||= on int-valued hashes must use has_key
# for the truthy check, not the raw _get return. C treats 0 as
# falsy but Ruby treats 0 as truthy, so the old _get guard
# incorrectly skipped &&= on stored 0 and incorrectly fired ||=
# on stored 0.

h = {"a" => 0, "b" => 1}
h["a"] &&= 99
h["b"] &&= 99
puts h["a"]
puts h["b"]

h2 = {"x" => 0, "y" => 1}
h2["x"] ||= 99
h2["y"] ||= 99
puts h2["x"]
puts h2["y"]

# SymIntHash variant
s = {a: 0, b: 1}
s[:a] &&= 99
s[:b] &&= 99
puts s[:a]
puts s[:b]

s2 = {x: 0, y: 1}
s2[:x] ||= 99
s2[:y] ||= 99
puts s2[:x]
puts s2[:y]

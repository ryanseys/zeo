# Float values stored in a Hash must survive a read by key. Before, a
# float-valued hash literal had no dedicated typed variant and fell
# through to the str_int_hash default, truncating the fractional part
# on read (52.9 -> 52). Float values now route to poly storage so each
# slot keeps its own tag.

# String key, float value
h = { "k" => 52.9 }
puts h["k"]

# Symbol key, float value
hs = { a: 1.5, b: 2.5 }
puts hs[:b]

# Integer key, float value
hi = { 0 => 1.5, 1 => 2.5 }
puts hi[1]

# Built then mutated keeps the fraction
hb = { "pi" => 3.14 }
hb["e"] = 2.71
puts hb["pi"]
puts hb["e"]

# Control: an int-valued hash is unaffected
hint = { "x" => 7 }
puts hint["x"]

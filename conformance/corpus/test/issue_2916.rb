# Hash literal whose only values are empty array literals (element kind
# unresolved) must still type to a concrete poly-valued hash variant.
p({ "empty" => [] })
h = { "a" => [1], "empty" => [] }
p h
p({ 1 => [] })
g = { "x" => [], "y" => [] }
g["x"] << 5
p g

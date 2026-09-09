# It is still a hash of arrays, alone and beside a non-empty one.
# (spinel issue #2916)
p({ "empty" => [] })
h = { "a" => [1], "empty" => [] }
p h
p({ 1 => [] })
g = { "x" => [], "y" => [] }
g["x"] << 5
p g
__END__
{"empty" => []}
{"a" => [1], "empty" => []}
{1 => []}
{"x" => [5], "y" => []}

# The block's writes land in the caller's hash.
# (spinel issue #2969)
h = {}
[1, 2, 3].each_with_object(h) { |x, acc| acc[x] = x * x }
p h
__END__
{1 => 1, 2 => 4, 3 => 9}

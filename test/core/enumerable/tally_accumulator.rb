# Enumerable#tally(hash) accumulates into the given hash and returns it (#2533)
h = Hash.new(0)
p([1, 1, 2, 3, 3, 3].tally(h))
p(h)
g = { 5 => 10 }
p([5, 5, 6].tally(g))            # accumulates onto existing counts
s = Hash.new(0)
p(%w[a a b].tally(s))
# plain tally still works (no arg)
p([1, 1, 2].tally)
__END__
{1 => 2, 2 => 1, 3 => 3}
{1 => 2, 2 => 1, 3 => 3}
{5 => 12, 6 => 1}
{"a" => 2, "b" => 1}
{1 => 2, 2 => 1}

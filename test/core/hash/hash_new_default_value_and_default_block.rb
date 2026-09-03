h = Hash.new(0)
h[:a] += 1
h[:a] += 1
p h[:a]
p h[:z]
p h.size
d = Hash.new { |hh, k| hh[k] = k.to_s }
p d[:q]
p d
__END__
2
0
1
"q"
{q: "q"}

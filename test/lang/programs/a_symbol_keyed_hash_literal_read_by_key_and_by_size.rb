# (spinel issue #3138)
p({x: 1})
h = {a: 1, b: 2, c: 3}
p h[:b]
p h.size
__END__
{x: 1}
2
3

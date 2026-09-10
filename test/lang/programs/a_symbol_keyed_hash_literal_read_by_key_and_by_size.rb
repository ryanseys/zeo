# A `{x: 1}` literal inspects with its symbol keys, answers `[]` by key, and
# counts its pairs.
p({x: 1})
h = {a: 1, b: 2, c: 3}
p h[:b]
p h.size
__END__
{x: 1}
2
3

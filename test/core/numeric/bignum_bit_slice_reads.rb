# A range of bits, single bits at and below the top, an offset and length pair,
# a range past the end, and an endless one.
p((2 ** 100)[10..20])
p((2 ** 100)[100])
p((2 ** 100)[99])
p((2 ** 100)[10, 5])
p((2 ** 100)[95..105])
p((2 ** 100)[100..])
p((2 ** 70)[68, 4])
__END__
0
1
0
0
32
1
4

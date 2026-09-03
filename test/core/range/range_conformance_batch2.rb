# Range conformance (KieranP #2411-#2415)
a = ("a".."e")
p a.begin
p a.end
p((5..1).min)
p((5..1).max)
p((5..1).minmax)
p((1...1).min)
p((1..5).min)
p((1..5).minmax)
p((1..).end)
p((1..5).entries)
p((1..10).step(3) { |x| }.class)
r = (1..5)
p r.begin
p r.end
__END__
"a"
"e"
nil
nil
[nil, nil]
nil
1
[1, 5]
nil
[1, 2, 3, 4, 5]
Range
1
5

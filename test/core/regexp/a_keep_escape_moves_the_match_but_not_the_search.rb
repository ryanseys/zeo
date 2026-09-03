r = Regexp.new('a\Kb')

p r =~ "ab"
p r.match("ab").begin(0)
p r.match("ab")[0]
p "ab".index(r)
p "ab" =~ r
p "ab".sub(r, "Z")

long = Regexp.new('xa\Kb')
p long =~ "xab"
p long.match("xab").begin(0)

p "ab".scan(r)
p "ab".split(Regexp.new('a\K'))
__END__
0
1
"b"
1
0
"aZ"
0
2
["b"]
["a", "b"]

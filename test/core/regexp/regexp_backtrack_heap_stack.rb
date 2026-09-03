[100, 5000, 9500, 10000, 12000, 20000].each do |n|
  s = "a" * n + "b" + "a" * n
  p [n, !!(s =~ /\A(a+)b\1\z/)]
end

p(/(a*)*\1/.match("aa").to_a)
p(/(a*)+\1/.match("aa").to_a)
p(/(a*)*\1/.match("").to_a)
p(/((a)*)\2/.match("aa").to_a)
p(/(?:(a)|(b))+\1/.match("ab").to_a)

p(/(?>a?)*/.match("b").to_a)
p(/(?>a*)*/.match("aab").to_a)
p(/(a?)\1*/.match("").to_a)

[200, 1000, 5000].each do |n|
  p [n, !!(("a" * n) =~ /\A(?>a)*\z/), !!(("a" * n) =~ /\A(?:(?=a)a)*\z/)]
end
[1, 50, 300].each do |d|
  p [d, !!("a" =~ Regexp.new("(?>" * d + "a" + ")" * d)),
        !!("a" =~ Regexp.new("(?=" * d + "a" + ")" * d + "a"))]
end

p(!!(("a" * 40 + "!") =~ /(a+)+$/))
p(!!(("a" * 40 + "!") =~ /(a*)*b\1/))

p(/(a)\1/.match("aa").to_a)
p(/(?>ab|a)*c/.match("abac").to_a)
p(/(a)(?=\1)/.match("aa").to_a)
p(/(a)(?!\1)/.match("ab").to_a)
p("aaa".scan(/(?>a)?/).size)
__END__
[100, true]
[5000, true]
[9500, true]
[10000, true]
[12000, true]
[20000, true]
["aa", ""]
["aa", ""]
["", ""]
["aa", "a", "a"]
[]
[""]
["aa"]
["", ""]
[200, true, true]
[1000, true, true]
[5000, true, true]
[1, true, true]
[50, true, true]
[300, true, true]
false
false
["aa", "a"]
["abac"]
["a", "a"]
["a", "a"]
4

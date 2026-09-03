# `^`-negated tr/squeeze sets, split's block form (yields fields, returns
# the receiver), and chomp("") paragraph mode.

p "abc".tr("^a", "x")
p "hello".tr("^aeiou", ".")
p "aaabbbccc".squeeze("^a")
p "aaa^^^bbb".squeeze("^")
r = []
"a,b,c".split(",") { |p| r << p.upcase }
p r
acc = 0
ret = "aa-bbb-c".split("-") { |p| acc += p.length }
p acc
p ret.equal?("aa-bbb-c".dup) == false && ret == "aa-bbb-c"
p "hello\r\n\r\n".chomp("")
p "hello\r".chomp("")
__END__
"axx"
".e..o"
"aaabc"
"aaa^bbb"
["A", "B", "C"]
6
true
"hello"
"hello\r"

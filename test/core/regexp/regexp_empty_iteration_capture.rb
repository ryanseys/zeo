# From the spinel corpus (fa06b601).
#
# A repetition whose body can match EMPTY runs one final iteration that
# consumes nothing, and CRuby's captured group holds the empty string at the
# position the loop stopped at. zeo's holds the last non-empty match instead,
# so the group and the overall match disagree about where the loop ended.
#
# The original header follows. It describes the PREDECESSOR project's
# version of this test and its own fix, not zeo's divergence above.
#
# A repetition whose body can match empty runs one final iteration that
# consumes nothing, and that iteration is the one that ends the loop: the group
# inside it holds the empty string at the position the loop stopped at. The VM
# stopped one iteration earlier, so the group still held the text of the last
# iteration that consumed something. Ported from mruby-regexp (45c588a83).
p "a".match(/(a?)*/)[1]
p "a".match(/(a?)*/).begin(1)
p "aab".match(/(a*)*b/)[1]
p "a".match(/(a?)+/)[1]
p "b".match(/(a?)*/)[1]
p "b".match(/(a*)*b/)[1]
p "a".match(/(|a)*/)[0]
p "ab".split(/(a?)*/, -1)
p "ab".scan(/(a?)*/)

# a body that always consumes keeps its single-pass walk
p "aaa".match(/(a)*/)[1]
p "aaa".match(/(a)+/)[0]
p "abc".scan(/x?/).size
p "xayb".scan(/(a|b)/).flatten
p "aaa".gsub(/a*/, "-")
__END__
""
1
""
""
""
""
""
["", "", "b", "", ""]
[[""], [""], [""]]
"a"
"aaa"
4
["a", "b"]
"--"

# CRuby stops a digit escape in a replacement from reaching a group as soon as
# the pattern declares a named one -- the same rule that stops a plain `(...)`
# from taking a number. Every digit escape was read as whatever group the
# number reached, so a replacement that named a group by number answered with
# the group's text. Ported from mruby-regexp 90aae07cf.
p "ab".sub(/(?<x>b)/, "[\\1]")
p "abab".gsub(/(?<x>b)/, "[\\1]")
p "ab".sub(/(?<a>a)(?<b>b)/, "[\\1\\2]")

# \k<name> is what reaches the group from a replacement
p "ab".sub(/(?<x>b)/, "[\\k<x>]")
p "abab".gsub(/(?<x>b)/, "[\\k<x>]")

# \0 stands for the whole match, and so do \& and \+, which stay outside the
# rule
p "ab".sub(/(?<x>b)/, "[\\0]")
p "ab".sub(/(?<x>b)/, "[\\&]")
p "ab".sub(/(?<x>b)/, "[\\+]")

# a pattern that names no group reads its digit escapes as before
p "ab".sub(/(a)(b)/, "[\\2\\1]")
p "abab".gsub(/(b)/, "[\\1]")

# the number a named group answers to is untouched where it is read
m = /(?<x>b)/.match("ab")
p m[1]
p m["x"]

# A character-set escape in a String method argument is not expanded, so
# the range collapses.
#
# A backslash in a tr/count/delete/squeeze character set escapes the next
# character, so "a\\-b" holds the three members a, - and b rather than the
# range a..b. The set parser had no escape at all.
p "a-b".tr("a\\-b", "*")
p "a-b".count("a\\-b")
p "a-b".delete("a\\-b")
p "abc".tr("a-c", "x")
p "a-b".tr("-", "_")
p "a\\b".count("\\\\")
p "abc".count("a-c")
p "hello".delete("l")
p "hello".squeeze("l")
p "hello".tr("el", "ip")
__END__
"***"
3
""
"xxx"
"a_b"
1
3
"heo"
"helo"
"hippo"

# Issue #858: `a-z` range notation inside the charset string
# argument of String#delete / #tr / #tr_s / #count expands to
# the codepoint range. Previously the range syntax was treated
# as three literal characters (a, -, z).
puts "hello world".delete("a-z").inspect
puts "abc123".tr("a-z", "X").inspect
puts "Hello World".count("a-z")
puts "AaBbCc".count("a-zA-Z")
puts "abc-def".delete("a-c").inspect

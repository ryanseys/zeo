# It removes one character, both bytes of a CRLF, and nothing from an empty
# string.
# (spinel issue #3085)
p "abcあ".chop
p "abc".chop
p "".chop
p "hello\r\n".chop
p "あいう".chop
__END__
"abc"
"ab"
""
"hello"
"あい"

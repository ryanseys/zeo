m = "abc123".match(/([a-z]+)(\d+)/)
p m.offset(0); p m.offset(1); p m.offset(2)
p m.byteoffset(0); p m.byteoffset(2)
p m.names
p m.regexp
n = "abc123".match(/(?<letters>[a-z]+)(?<digits>\d+)/)
p n.offset(:letters); p n.names
p Regexp.union("a+b", /c.d/, "e")
p Regexp.union("a+b", /c.d/, "e").source
p Regexp.union(["x", "y"]).source
p Regexp.union.source
p Regexp.linear_time?(/a*/)
p Regexp.linear_time?(/(a+)+/)
p Regexp.try_convert(/x/); p Regexp.try_convert("x"); p Regexp.try_convert(42)
__END__
[0, 6]
[0, 3]
[3, 6]
[0, 6]
[3, 6]
[]
/([a-z]+)(\d+)/
[0, 3]
["letters", "digits"]
/a\+b|(?-mix:c.d)|e/
"a\\+b|(?-mix:c.d)|e"
"x|y"
"(?!)"
true
true
/x/
nil
nil

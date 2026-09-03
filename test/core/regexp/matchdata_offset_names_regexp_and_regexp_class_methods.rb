m = "abc123".match(/([a-z]+)(\d+)/)
p m.offset(2)
p m.byteoffset(2)
p "x1".match(/(?<a>[a-z])(?<b>\d)/).names
p Regexp.union("a", "b").source
p Regexp.union.source
p Regexp.linear_time?(/(a+)+/)
p Regexp.linear_time?(/(a)\1/)
p Regexp.try_convert("x")
__END__
[3, 6]
[3, 6]
["a", "b"]
"a|b"
"(?!)"
true
false
nil

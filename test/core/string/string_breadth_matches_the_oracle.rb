# The String Tier A surface: case/strip families, split shapes, chomp,
# indexing forms, sub/gsub (String + block), tr/delete/squeeze/count,
# lenient conversions, succ carry, padding, and `%` formatting.

p "hello world".capitalize
p "HeLLo".swapcase
p "hello".upcase
p "HELLO".downcase
p "  hi  ".strip
p "  hi".lstrip
p "hi  ".rstrip
p "hello".chars
p "a,b,,c".split(",")
p "a b  c".split
p "hello".split("l")
p "hello".chomp("lo")
p "hello".chop
p "abc" * 3
p "abc".reverse
p "hello".index("l")
p "hello".rindex("l")
p "hello".index("x")
p "hello"[1]
p "hello"[1, 3]
p "hello"[1..3]
p "hello".sub("l", "L")
p "hello".gsub("l", "L")
p "hello".gsub("l") { |m| m.upcase }
p "hello".start_with?("he")
p "hello".end_with?("lo", "x")
p "hello".tr("el", "ip")
p "hello".tr("a-y", "b-z")
p "42abc".to_i
p "abc".to_i
p "ff".to_i(16)
p "42.5xyz".to_f
p "hello".to_sym
p "az".succ
p "zz".succ
p "a\nb\nc".lines
p "Hello %s, you are %d" % ["Bob", 42]
p "%05.1f|%x|%o|%b|%e|%g|%%" % [3.14159, 255, 8, 5, 12345.678, 0.00001]
p "hi".center(7, "*")
p "hi".ljust(5, ".")
p "hi".rjust(5, ".")
p "hello".delete("l")
p "aabbcc".squeeze
p "aabbcc".squeeze("a")
p "hello world".count("lo")
s = "orig"
s.replace("xyz")
p s
s << "!"
p s
s.prepend("ab")
p s
__END__
"Hello world"
"hEllO"
"HELLO"
"hello"
"hi"
"hi"
"hi"
["h", "e", "l", "l", "o"]
["a", "b", "", "c"]
["a", "b", "c"]
["he", "", "o"]
"hel"
"hell"
"abcabcabc"
"cba"
2
3
nil
"e"
"ell"
"ell"
"heLlo"
"heLLo"
"heLLo"
true
true
"hippo"
"ifmmp"
42
0
255
42.5
:hello
"ba"
"aaa"
["a\n", "b\n", "c"]
"Hello Bob, you are 42"
"003.1|ff|10|101|1.234568e+04|1e-05|%"
"**hi***"
"hi..."
"...hi"
"heo"
"abc"
"abbcc"
5
"xyz"
"xyz!"
"abxyz!"

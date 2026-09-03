p "ab".sub(/(?<x>b)/, "[\\1]")
p "abab".gsub(/(?<x>b)/, "[\\1]")
p "ab".sub(/(?<a>a)(?<b>b)/, "[\\1\\2]")

p "ab".sub(/(?<x>b)/, "[\\k<x>]")
p "abab".gsub(/(?<x>b)/, "[\\k<x>]")

p "ab".sub(/(?<x>b)/, "[\\0]")
p "ab".sub(/(?<x>b)/, "[\\&]")
p "ab".sub(/(?<x>b)/, "[\\+]")

p "ab".sub(/(a)(b)/, "[\\2\\1]")
p "abab".gsub(/(b)/, "[\\1]")

m = /(?<x>b)/.match("ab")
p m[1]
p m["x"]
__END__
"a[]"
"a[]a[]"
"[]"
"a[b]"
"a[b]a[b]"
"a[b]"
"a[b]"
"a[b]"
"[ba]"
"a[b]a[b]"
"b"
"b"

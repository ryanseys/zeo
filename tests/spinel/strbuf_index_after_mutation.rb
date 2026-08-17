s = String.new
s << "abc"
p s[0], s[2], s.length
s << "de"
p s[3], s[4], s[5], s.length
s.insert(0, "Z")
p s[0], s[1], s.length
s.replace("hello")
p s[0], s[4], s.length, s[1, 3], s[1..3]

# a multibyte buffer indexes by character, not by byte
u = String.new
u << "あい"
p u[0], u[1], u.length, u.bytesize
u << "う"
p u[2], u.length

# the last match's $` and $' survive being read after other work
"hello world" =~ /o w/
mid = [$`, $&, $']
p mid
p "aXbXc".gsub(/X/) { "-" }
p "one two three".sub(/two/) { |m| m.upcase }
p [$`, $']

# A named group has two spellings and the engine read both of them in \k while
# accepting only one in a declaration: the scan that decides whether a plain
# group still captures already knew (?'name'), so a pattern using it demoted
# its plain groups and then fell through to a stray `?`.
m = "abc".match(/(?'x'b)/)
p [m[:x], m[0], m.size]

p "abc".match(/(?<y>b)/)[:y]
p "aa".match(/(?'z'a)\k'z'/)[0]
p "aa".match(/(?'z'a)\k<z>/)[0]
p "ab".match(/(a)(?'b'b)/).size
p("x'y" =~ /(?<a'b>x'y)/ ? $~[:"a'b"] : nil)

begin
  Regexp.new("(?'x'a")
rescue => e
  p e.class
end
begin
  Regexp.new("(?<>a)")
rescue => e
  p e.class
end
begin
  Regexp.new("(?''a)")
rescue => e
  p e.class
end

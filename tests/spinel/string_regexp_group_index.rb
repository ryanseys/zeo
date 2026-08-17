# The Regexp-with-capture-group form of the String index family: `s[/re/, n]`
# already read the group, but removing or replacing it had no arm at all --
# slice! answered nil and left the receiver alone, and []= was refused.
s = +"hello"
p s.slice!(/(l)(l)/, 2)
p s

# the group's own span, not the first textual occurrence of its text: those
# are different characters when the group repeats
t = +"abab"
p t.slice!(/(a)(b)(a)(b)/, 3)
p t

u = +"hello"
p u.slice!(/l+/, 0)
p u

v = +"hello"
v[/(l)(o)/, 2] = "O"
p v

w = +"abab"
w[/(a)(b)(a)/, 3] = "X"
p w

y = +"hello"
y[/l+/, 0] = "L"
p y

z = +"hello"
begin
  z[/zz/, 1] = "q"
rescue IndexError => e
  p e.class
end
p z

# the reading forms keep working
p "hello"[/(l)(o)/, 2]
p "hello"[/l+/]

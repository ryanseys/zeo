# `s[i] = v` took a statically Integer index only. A boxed one reaches it
# whenever the value came out of a container -- a destructured block parameter,
# an element read, an untyped argument -- and the call fell through to
# "undefined method '[]=' for an instance of String" on a program CRuby runs
# (#4060). The conversion is the checked one, so a boxed Integer works and the
# splice forms spinel does not support raise TypeError rather than being read
# as a number.
s = +"abc"
pairs = [[0, 1]]
pairs.each { |r, c| s[c] = "*" }
p s

t = +"hello"
idx = [[0, 1], [2, 3]][1][0]
t[idx] = "L"
p t

def write_at(str, i)
  str[i] = "@"
  str
end
p write_at(+"abcd", 2)

# a negative boxed index counts from the end, and one past the end appends
u = +"xyz"
neg = [-1].first
u[neg] = "Z"
p u
v = +"ab"
at_end = [2].first
v[at_end] = "c"
p v

# out of range still raises IndexError with the original index
w = +"ab"
far = [9].first
begin
  w[far] = "!"
rescue IndexError => e
  p e.message
end

# A method appending to its parameter changes the caller's string, and an alias
# taken through a reader outside the class does too.
def scream(x)
  x << "!"
end
def outer(y)
  scream(y)
  y << "?"
end

s = +"a"
t = s
s << +"b"
scream(s)
p s
p t
p s.equal?(t)

u = +"plain"
scream(u)
p u

v = +"deep"
w = v
v << "-"
outer(v)
p v
p w

scream(+"lit")

class Doc
  attr_reader :title
  def initialize(t) = @title = t
  def bump = @title << "+"
end
d = Doc.new(+"aaa")
ext = d.title
d.bump
p ext
p ext.equal?(d.title)

class Memo
  def initialize = @body = +"b"
  def body = @body
  def grow = @body << "1"
end
m = Memo.new
x = m.body
m.grow
p x
__END__
"ab!"
"ab!"
true
"plain!"
"deep-!?"
"deep-!?"
"aaa+"
true
"b1"

# An array seeded with one element and cleared in initialize, pushed to, and
# iterated; and a hash cleared through a method that returns it.
# (spinel issue #3326)
class Res
  attr_accessor :cookies
  def initialize
    @cookies = [""]
    @cookies.clear
  end

  def add(name, value)
    @cookies.push(name + "=" + value)
  end
end

res = Res.new
res.add("flash", "hello")
head = +""
res.cookies.each do |line|
  head << "Set-Cookie: " + line + "\r\n"
end
print head

h = { a: 1 }
def touch(x) = x
v = touch(h)
v.clear
p h
__END__
Set-Cookie: flash=hello
{}

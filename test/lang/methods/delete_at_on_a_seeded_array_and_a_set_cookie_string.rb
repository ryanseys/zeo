# An array seeded with one element and emptied by delete_at, then filled, and
# a "Set-Cookie" string beside it.
# (spinel issue #3298)
class R
  attr_accessor :cookies
  def initialize
    @cookies = [+""]
    @cookies.delete_at(0)
  end
  def add(line) = @cookies.push(line)
end
r = R.new
r.add("a=1")
r.add("b=2")
head = +""
i = 0
while i < r.cookies.length
  head << "Set-Cookie: " + r.cookies[i] + "\r\n"
  i += 1
end
puts head
p r.cookies.delete_at(0)
p r.cookies
p r.cookies.delete_at(5)
__END__
Set-Cookie: a=1
Set-Cookie: b=2
"a=1"
["b=2"]
nil

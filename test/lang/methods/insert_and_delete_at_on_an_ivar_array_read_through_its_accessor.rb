# insert at 0 and -1 through the reader, a while loop building a string from
# the elements, and delete_at in and out of range.
class R
  attr_accessor :cookies
  def initialize
    @cookies = [""]
    @cookies.delete_at(0)
  end

  def add(line)
    @cookies.push(line)
  end
end

r = R.new
r.add("a=1")
r.cookies.insert(0, "z=9")
r.cookies.insert(-1, "t=2")
head = +""
i = 0
while i < r.cookies.length
  head << "Set-Cookie: " + r.cookies[i] + "\r\n"
  i += 1
end
puts head
p r.cookies.delete_at(0)
p r.cookies.delete_at(5)
p r.cookies
__END__
Set-Cookie: z=9
Set-Cookie: a=1
Set-Cookie: t=2
"z=9"
nil
["a=1", "t=2"]

# Two bugs from the Rails-emit boot crash: a poly-array receiver had no
# delete_at arm, and "Set-Cookie" in a string tripped the textual Set
# auto-require, whose top-of-buffer prepend pushed this file's magic
# comment off the scanned lines (reverting the pragma to the default).
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

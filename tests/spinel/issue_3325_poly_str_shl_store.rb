class Res
  attr_accessor :cookies
  def initialize
    @cookies = [""]
    @cookies.pop
  end

  def add(name, value)
    line = name + "=" + value
    line << "; HttpOnly"
    @cookies.push(line)
  end
end

res = Res.new
res.add("flash", "hello")
head = +""
res.cookies.each do |line|
  head << "Set-Cookie: " + line + "\r\n"
end
print head

class Res2
  attr_accessor :cookies
  def initialize
    @cookies = [""]
    @cookies.pop
  end

  def add(name, value)
    line = name + "=" + value
    ["; Path=/", "; HttpOnly", "; SameSite=Lax"].each { |a| line << a }
    @cookies.push(line)
  end
end

r2 = Res2.new
r2.add("sid", "42")
puts r2.cookies[0]

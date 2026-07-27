class Res
  attr_accessor :cookies
  def initialize
    @cookies = [""]
    @cookies.pop
  end

  def store(name, value)
    line = name + "=" + value
    line << "; HttpOnly"
    @cookies.push(line)
  end

  def set_cookie(name, value)
    line = name + "=" + value
    line << "; Path=/"
    @cookies.push(line)
  end

  def apply_cookie(name, value)
    line = name + "=" + value
    line << "; Secure"
    @cookies.push(line)
  end
end

res = Res.new
res.store("flash", "hello")
res.set_cookie("sid", "42")
res.apply_cookie("theme", "dark")
res.cookies.each { |line| puts "Set-Cookie: " + line }

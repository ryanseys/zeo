class Greeter
  def greet(name, greeting = "Hello")
    "#{greeting}, #{name}!"
  end
end
g = Greeter.new
puts g.greet("Ada")
puts g.greet("Ada", "Hi")
__END__
Hello, Ada!
Hi, Ada!

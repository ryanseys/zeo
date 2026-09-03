# Callable by implicit self everywhere, invisible to `respond_to?`,
# reachable via `send`, and a NoMethodError with an explicit receiver.

def greet(name)
  "Hello, #{name}!"
end
class Speaker
  def speak
    greet("instance")
  end
end
puts greet("top")
puts Speaker.new.speak
p self.respond_to?(:greet)
p self.respond_to?(:greet, true)
p Speaker.new.respond_to?(:greet)
p send(:greet, "send")
p Speaker.new.send(:greet, "on-instance")
begin
  Speaker.new.greet("explicit")
rescue NoMethodError
  puts "NoMethodError"
end
__END__
Hello, top!
Hello, instance!
false
true
false
"Hello, send!"
"Hello, on-instance!"
NoMethodError

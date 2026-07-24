class Greeter
  def greet(text = nil, &block)
    puts text || "anonymous"
    yield if block
  end

  def call_internal
    greet("internal")
  end
end

Greeter.new.greet("hi")
Greeter.new.call_internal

def forwarder(&block)
  Greeter.new.greet("forwarded", &block)
end
forwarder { puts "  block received" }

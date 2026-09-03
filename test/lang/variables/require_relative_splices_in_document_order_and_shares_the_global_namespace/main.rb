puts "before require"
require_relative "greeter"
puts "after require"
g = Greeter.new
puts g.greet("world")
puts GREETING

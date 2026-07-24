# Class#< with a built-in parent class. A user class A < StandardError
# should report `A < StandardError == true` and have StandardError
# in its ancestor chain.

class A < StandardError; end
class B < Numeric; end

puts((A < StandardError).inspect)
puts((A <= StandardError).inspect)
puts((B < Numeric).inspect)
puts((B < Object).inspect)
# Sibling hierarchies: A < Object (true via StandardError -> Object).
puts((A < Object).inspect)

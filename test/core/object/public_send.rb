# `public_send` reaches a public method by name, given either a Symbol or a
# String, and `__send__` does the same. Whether public_send REFUSES a private
# method is a separate question with its own tests.

puts "hello".public_send(:upcase)
puts "world".public_send("upcase")
puts 42.public_send(:to_s)

# __send__ also keeps working (existing path).
puts "x".__send__(:upcase)
__END__
HELLO
WORLD
42
X

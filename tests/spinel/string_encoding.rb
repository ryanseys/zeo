# Issue #723. `.encoding` returns the source label (spinel uses
# UTF-8 throughout) as a small Encoding value.

puts "hello".encoding
puts "x".encode.encoding
puts "y".b.encoding
puts "hello".encoding.class
puts "hello".encoding.name

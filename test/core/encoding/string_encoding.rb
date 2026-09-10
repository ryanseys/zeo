# `.encoding` answers an Encoding value naming the string's encoding.

puts "hello".encoding
puts "x".encode.encoding
puts "y".b.encoding
puts "hello".encoding.class
puts "hello".encoding.name
__END__
UTF-8
UTF-8
ASCII-8BIT
Encoding
UTF-8

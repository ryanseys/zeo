puts "hello".match?(/l+/)
puts "hello".match?(/xyz/)
puts(/l+/.match?("hello"))
__END__
true
false
true

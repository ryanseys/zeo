begin
  puts 255[..3]
rescue ArgumentError => e
  puts "AE: #{e.message}"
end
a = 255
begin
  puts a[..3]
rescue ArgumentError => e
  puts "AE: #{e.message}"
end
puts 255[0..3]
puts 255[4..]
puts 255[1, 3]
__END__
AE: The beginless range for Integer#[] results in infinity
AE: The beginless range for Integer#[] results in infinity
15
15
7

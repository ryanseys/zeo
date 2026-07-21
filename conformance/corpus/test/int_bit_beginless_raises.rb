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

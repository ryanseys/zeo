begin
  (..5).each { |x| x }
  puts "no-raise-1"
rescue TypeError => e
  puts e.message
end
begin
  (...5).each { |x| x }
  puts "no-raise-2"
rescue TypeError => e
  puts e.message
end
(1..3).each { |x| print x }
puts

# (..5).each cannot start anywhere, so it raises, and the message says so.
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
__END__
can't iterate from NilClass
can't iterate from NilClass
123

begin
  raise RangeError, "oops"
rescue ArgumentError
  puts "arg"
rescue TypeError
  puts "type"
rescue RangeError => e
  puts "range: #{e.send(:message)}"
end
__END__
range: oops

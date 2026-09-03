begin
  raise TypeError, "wrong type"
rescue ArgumentError, TypeError => e
  puts "caught one of: #{e.send(:message)}"
end
__END__
caught one of: wrong type

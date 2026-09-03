begin
  puts "try"
  raise ArgumentError, "bad"
rescue ArgumentError => e
  puts "caught: #{e.send(:message)}"
ensure
  puts "ensure ran"
end
__END__
try
caught: bad
ensure ran

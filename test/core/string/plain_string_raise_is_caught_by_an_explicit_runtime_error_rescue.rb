begin
  raise "just a string"
rescue RuntimeError => e
  puts "caught runtime: #{e.send(:message)}"
end
__END__
caught runtime: just a string

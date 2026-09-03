puts "hello world".gsub(/o/, "0")
puts "hello world".sub(/o/, "0")
puts "John Smith".gsub(/(\w+) (\w+)/, '\2 \1')
__END__
hell0 w0rld
hell0 world
Smith John

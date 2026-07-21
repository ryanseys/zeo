# String#chomp edge cases: no-arg strips ONE separator,
# paragraph mode (chomp("")) strips trailing \r\n/\n sequences
# but not standalone \r.

# No-arg chomp: one separator
puts "hello\r\n".chomp.inspect
puts "hello\n".chomp.inspect
puts "hello\r".chomp.inspect
puts "hello\r\n\r\n".chomp.inspect
puts "hello".chomp.inspect

# Paragraph mode
puts "hello\r\n".chomp("").inspect
puts "hello\n".chomp("").inspect
puts "hello\r".chomp("").inspect
puts "hello\r\n\r\n".chomp("").inspect

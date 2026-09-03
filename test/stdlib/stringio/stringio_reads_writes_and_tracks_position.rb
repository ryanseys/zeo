require "stringio"
io = StringIO.new
io.puts "hello"
io.print "world"
puts io.string.inspect
io.rewind
puts io.gets.inspect
puts io.read.inspect
__END__
"hello\nworld"
"hello\n"
"world"

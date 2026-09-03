STDOUT.puts "via const"
$stdout.puts "via global"
STDERR.puts "err const"
$stderr.print "err global\n"
puts STDOUT.write("abc\n")
$stdout = STDERR
puts "redirected"
$stdout = STDOUT
puts "back"
puts STDOUT.inspect
__END__
via const
via global
abc
4
back
#<IO:<STDOUT>>
#@ stderr
err const
err global
redirected

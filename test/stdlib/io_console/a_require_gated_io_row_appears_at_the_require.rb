puts STDOUT.respond_to?(:getch)
puts STDOUT.respond_to?(:winsize)
puts STDOUT.respond_to?(:nonblock?)
puts IO.respond_to?(:console)
require "io/console"
puts STDOUT.respond_to?(:getch)
puts IO.respond_to?(:console)
__END__
false
false
false
false
true
true

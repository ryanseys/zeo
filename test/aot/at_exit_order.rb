# at_exit handlers run in reverse order of registration, after the main body,
# in a linked binary as in ruby.
at_exit { puts "first registered, last run" }
at_exit { puts "second registered, first run" }
puts "body"
__END__
body
second registered, first run
first registered, last run

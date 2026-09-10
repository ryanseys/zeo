# at_exit hooks run in LIFO order after main returns.
at_exit { puts "first registered" }
at_exit { puts "second registered" }
at_exit { puts "third registered" }
puts "main"
__END__
main
third registered
second registered
first registered

# `return` written at the top level ends the program. Zeo used to fold only
# the plain statement form; from a position that RAISES the signal instead of
# returning literally -- inside a `rescue` clause, an `ensure`, a `begin/end
# while` -- it escaped every frame and the program died with "uncaught signal
# escaped the top level". The exit status is 0 and `at_exit` still runs, both
# of which say this is an ordinary end and not an error.
at_exit { puts "at_exit ran" }

puts "start"

begin
  raise "boom"
rescue RuntimeError => e
  puts "rescued #{e.message}"
  return
end

puts "never printed"
__END__
start
rescued boom
at_exit ran

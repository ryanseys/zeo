# Issue #854: String#partition with regex used to SEGV (the
# string-only path passed the regex pattern pointer to strstr);
# `puts <tuple>` with a partition result printed the struct
# pointer as an integer instead of splatting elements.
puts "hello world".partition(" ")
puts "hello world".partition(/o/).inspect
puts "hello,world".partition(",").inspect
puts "no-sep".partition(",").inspect

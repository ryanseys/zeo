# `system` inherits stdio and answers true (exit 0) / false (nonzero),
# setting `$?` either way. A nonzero exit does not raise.

p system("true")
puts $?.exitstatus
p system("false")
puts $?.success?
puts $?.exitstatus
__END__
true
0
false
false
1

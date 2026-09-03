# `$LOAD_PATH` is still nil when the program's first statement runs: zeo's
# preamble initializes it after user code rather than before it, so `unshift`
# raises on a nil receiver. ruby has it populated before line 1 executes.
$:.unshift File.dirname(__FILE__)
$LOAD_PATH.unshift(File.expand_path("../lib", __FILE__))
$LOAD_PATH << File.dirname(__FILE__)
$:.push File.dirname(__FILE__)
puts 1
__END__
1

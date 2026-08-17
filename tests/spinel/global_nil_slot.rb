# A global whose only value is nil got no type, and a global with no type got
# no slot, so the reference did not compile at all ('gv_foo' undeclared). The
# same for a global never assigned anywhere, which Ruby also reads as nil.
$foo = nil
p $foo.nil?
p $foo

# never assigned
p $never_written.nil?

# a later write still decides the type: this one is an Integer, not a boxed slot
$n = nil
$n = 5
p $n
p $n + 1

# and a nil string global is nil, not the empty string: assigning nil used to
# store "" while `.nil?` compares against NULL, so it answered false
$s = nil
p $s.nil?
$s = "x"
p $s.nil?
p $s

# read through a method, which is where this first showed up
$log = nil
$log_dead = false

def live_write
  puts "no pipe" if $log.nil?
  puts "dead" if $log_dead
end

live_write
$log = "open"
live_write
p $log

# the underscore-prefixed spelling takes the same path
$_x = nil
p $_x.nil?

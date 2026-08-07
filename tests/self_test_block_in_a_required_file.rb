# `if __FILE__ == $0` -- the self-test block every second library file ends
# with. In a file something else REQUIRED it is false whatever `$0` holds: the
# entry point is, by definition, a different file. The definitions it guards
# must therefore not register, which is what lets a library that ships its own
# test harness compile at all.
require_relative "self_test_block_in_a_required_file/lib"

p Memcache.new.get
p defined?(TestConnection)

# `unless Socket.const_defined?(:AF_INET6)` drops an IPv6 fallback definition on
# a platform that already defines it (the rubygems/ipaddr shape). zeo folds the
# guard at compile time against the seeded Socket constants.
require "socket"

unless Socket.const_defined?(:AF_INET6)
  raise "fallback should be dead code on an IPv6 platform"
end

puts Socket.const_defined?(:AF_INET6)
puts Socket::AF_INET6 == Socket::AF_INET6
puts Socket::SOCK_STREAM.class

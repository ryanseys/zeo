# Persistent registration: register once per connection, then read only what
# fired. The older reset/add/run/ready contract rebuilds the set every tick, so
# a caller with N parked fds does O(N) work per tick however few are ready;
# these cost O(events) instead. (matz/spinel#4103)
#
# fd 1 is this process's stdout: always open and always write-ready, whether it
# is a file or a pipe with room, so the readiness assertion below holds on any
# POSIX target without binding a port.
module Net
  ffi_func :sp_net_poll_register,   [:int, :int], :int
  ffi_func :sp_net_poll_modify,     [:int, :int], :int
  ffi_func :sp_net_poll_unregister, [:int],       :int
  ffi_func :sp_net_poll_registered, [],           :int
  ffi_func :sp_net_poll_wait,       [:int],       :int
  ffi_func :sp_net_poll_event_fd,   [:int],       :int
  ffi_func :sp_net_poll_event_mode, [:int],       :int
end

# Registering keys on the fd: twice is once.
puts "register: #{Net.sp_net_poll_register(1, 2)}"
puts "again: #{Net.sp_net_poll_register(1, 2)}"
puts "registered: #{Net.sp_net_poll_registered}"

# stdout is write-ready, and the event names the fd and the mode.
n = Net.sp_net_poll_wait(0)
puts "ready: #{n}"
puts "event fd: #{Net.sp_net_poll_event_fd(0)}"
puts "event mode: #{Net.sp_net_poll_event_mode(0)}"

# Reading past what fired answers "nothing", rather than walking off the set.
puts "beyond fd: #{Net.sp_net_poll_event_fd(n)}"
puts "beyond mode: #{Net.sp_net_poll_event_mode(n)}"
puts "negative fd: #{Net.sp_net_poll_event_fd(-1)}"
puts "negative mode: #{Net.sp_net_poll_event_mode(-1)}"

# Interest changes in place, without a second registration.
puts "modify: #{Net.sp_net_poll_modify(1, 1)}"
puts "registered: #{Net.sp_net_poll_registered}"

# An fd that was never registered is refused by both mutators.
puts "modify absent: #{Net.sp_net_poll_modify(4242, 1)}"
puts "unreg absent: #{Net.sp_net_poll_unregister(4242)}"
puts "register bad fd: #{Net.sp_net_poll_register(-1, 1)}"

# The set grows and shrinks with the registrations, well past the 256 the
# older set was capped at.
(3..600).each { |fd| Net.sp_net_poll_register(fd, 1) }
puts "many: #{Net.sp_net_poll_registered}"
(3..600).each { |fd| Net.sp_net_poll_unregister(fd) }
puts "back to: #{Net.sp_net_poll_registered}"

# Removing the last one empties it.
puts "unregister: #{Net.sp_net_poll_unregister(1)}"
puts "registered: #{Net.sp_net_poll_registered}"

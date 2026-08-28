# The readiness set grows with the caller's connection count.
#
# It used to be a fixed 256-entry array whose overflow answered -1 -- and -1 is
# also what a caller reads as "this fd is not in the set this round", so from
# the 257th fd on a connection went deaf with nothing said about it. A server
# holding one socket per user reaches 256 on its first busy minute.
#
# poll_add only records (fd, mode); it does not touch the fd, so the same fd
# registered many times is a faithful test of the set's capacity.
module Net
  ffi_func :sp_net_poll_reset, [],          :int
  ffi_func :sp_net_poll_add,   [:int, :int], :int
  ffi_func :sp_net_poll_ready, [:int],       :int
end

Net.sp_net_poll_reset

# Slots are handed out in order, and keep being handed out well past the old
# 256 ceiling.
slots = []
1000.times { slots << Net.sp_net_poll_add(0, 1) }

puts "first: #{slots[0]}"
puts "at old cap: #{slots[255]}"
puts "past old cap: #{slots[256]}"
puts "last: #{slots[999]}"
puts "any refused: #{slots.any? { |s| s < 0 }}"
puts "all in order: #{slots.each_with_index.all? { |s, i| s == i }}"

# A slot past the registered count reads as "nothing fired" rather than
# walking off the set.
puts "beyond set: #{Net.sp_net_poll_ready(5000)}"
puts "negative: #{Net.sp_net_poll_ready(-1)}"

# reset re-uses the same storage: the next add starts at slot 0 again.
Net.sp_net_poll_reset
puts "after reset: #{Net.sp_net_poll_add(0, 1)}"

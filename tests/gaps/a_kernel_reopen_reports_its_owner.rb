# A `def` written inside `module Kernel` is owned by Kernel, and every
# reflection route says so. zeo answers the CARRIER instead -- the class the
# materialized row landed on -- so the answer even varies by receiver.
#
#   5.method(:tagged).owner        ruby: Kernel   zeo: Integer
#   Widget.instance_method(...)    ruby: Kernel   zeo: Object
#
# Found while giving a Kernel reopen ONE emitted body (the carriers now
# share it, see `a_kernel_reopen_is_one_body_for_every_carrier`); this is
# the row that could not be pinned there. It is INDEPENDENT of that work --
# the same answers come out with the sharing turned off.
#
# `Module#include?` and `is_a?` already answer correctly, so the ancestry is
# right and only the row's recorded owner is wrong.

module Kernel
  def tagged(x) = x
end

class Widget; end

p Widget.new.method(:tagged).owner
p 5.method(:tagged).owner
p "s".method(:tagged).owner
p Widget.instance_method(:tagged).owner
p Integer.instance_method(:tagged).owner

# The ancestry these answers are supposed to be read off is already right.
p [Widget.include?(Kernel), Integer.include?(Kernel), Widget.new.is_a?(Kernel)]
p Kernel.instance_method(:tagged).owner

# A definition on the universal spine -- Object, Kernel, BasicObject -- is
# inherited by every class in the program, and zeo emits its body ONCE rather
# than once per class. This pins the four things that makes observable.
#
# An ivar a shared body touches must land on the RECEIVER, a constant it reads
# must resolve against Object, `self` must be the receiver, and the private
# visibility a top-level `def` carries in ruby must survive on every carrier.

CONST = "toplevel-const"

def stash(v) = @stashed = v
def fetch_stashed = @stashed
def read_const = CONST
def which_self = self.class.name

module Kernel
  def kernel_reopened = "kernel-#{self.class}"
end

class Widget
  ONLY_HERE = "widget-const"
  def initialize = @own = 1

  def use_all
    stash(:from_widget)
    [fetch_stashed, read_const, which_self, kernel_reopened, @own, ONLY_HERE]
  end
end

class Gadget < Widget
  def initialize
    super
    @extra = 2
  end

  def peek
    stash(:from_gadget)
    [fetch_stashed, @own, @extra, read_const]
  end
end

widget = Widget.new
p widget.use_all
gadget = Gadget.new
p gadget.peek

# The ivar the shared body wrote belongs to the receiver, not to Object, and
# each receiver keeps its own.
p [widget.instance_variables.sort, gadget.instance_variables.sort]

# A top-level `def` is PRIVATE, on every class that inherits it.
p [widget.respond_to?(:read_const), widget.private_methods.include?(:read_const)]
p [widget.send(:read_const), 1.send(:read_const), "s".send(:read_const)]

# Reflection still names the real owner rather than the carrier.
p [widget.method(:read_const).owner, Widget.instance_method(:read_const).owner]
p [widget.is_a?(Kernel), Widget.include?(Kernel)]

# A reopened Kernel reaches every class the same way, and `self` inside it is
# the receiver.
p [widget.send(:kernel_reopened), gadget.send(:kernel_reopened), 1.send(:kernel_reopened)]

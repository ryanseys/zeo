# A `def` written inside `module Kernel` reaches every class in the program,
# and zeo emits its body ONCE. This is the same rule as a top-level `def`
# (see `a_toplevel_def_is_one_body_for_every_class`), reached by the other
# spelling -- and the spelling mattered: the spine lookup asks a map that
# `clif::collect::collect_methods` fills from TOP-LEVEL defs only, so a
# `def` inside `module Kernel` was never in it and every builtin carrier
# took a private copy. `Kernel#URI` cost 85 of them on a program that only
# requires uri.
#
# The carriers here are deliberately of every kind: a user class, an
# immediate, a builtin with a payload, and an exception.

module Kernel
  def tagged(x) = "#{self.class}:#{x}"
  def reads_toplevel = TOP

  private

  def hushed = "quiet"
end

TOP = "top-level"

class Widget; end

w = Widget.new
p [w.tagged(1), 5.tagged(2), "s".tagged(3), [1].tagged(4), :sym.tagged(5), nil.tagged(6)]
p [w.reads_toplevel, 5.reads_toplevel]

begin
  raise ArgumentError, "boom"
rescue => e
  p [e.tagged(7), e.reads_toplevel]
end

# `Kernel`'s `private` reaches every carrier, and none of them answers
# `respond_to?`.
p [w.respond_to?(:hushed), 5.respond_to?(:hushed), "s".respond_to?(:hushed)]
p [w.send(:hushed), 5.send(:hushed)]

# Every carrier really does reach it through Kernel.
p [Widget.include?(Kernel), Integer.include?(Kernel), String.include?(Kernel)]
p [w.is_a?(Kernel), 5.is_a?(Kernel)]
# `Method#owner` answers the CARRIER here, not Kernel -- a separate,
# pre-existing divergence with its own gap file
# (`a_kernel_reopen_reports_its_owner`), deliberately not printed.

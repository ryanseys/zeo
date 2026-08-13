# `Module#undef_method` called at RUNTIME -- from inside a `def self.x` body,
# where `self` is the module -- retires the name for every class that already
# includes the module. zeo keeps answering with the original body.
#
# The name IS collected: `analyze::collect_runtime_undefs` descends into a
# `def self.x` body precisely so this shape is seen (a `self` that is the
# module means the undef really does target it), and the name lands on
# `ClassInfo::runtime_undefs`, which stops codegen emitting a DIRECT call.
# So the call site correctly goes through dynamic dispatch.
#
# What is missing is the runtime half: the dispatched `undef_method` has to
# write a TOMBSTONE into the overlay that the MRO walk then honours for every
# includer, the way `zeo_rt`'s dynamic definition path installs a body. Today
# the send resolves and does nothing that a later lookup consults, so the
# static row keeps winning.
#
# The gate is `Module#undef_method` reaching a module that has already been
# included -- a plain `undef_method` in a class BODY is decided at compile
# time and works (see `tests/a_nested_class_undef_stays_in_that_class.rb`).
module Retiring
  def self.retire
    undef_method :doomed
  end

  def doomed
    "still here"
  end
end

class UsesRetiring
  include Retiring
end

p UsesRetiring.new.doomed
Retiring.retire
begin
  p UsesRetiring.new.doomed
rescue NoMethodError
  p :doomed_undefined
end

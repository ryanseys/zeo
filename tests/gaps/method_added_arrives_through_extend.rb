# GAP: `method_added` reaches a class through `extend` and never fires.
#
# The hook works when it is INHERITED (`def self.method_added` on a
# superclass). It does not work when it arrives the other standard way -- a
# module's `included` hook doing `base.extend(ClassMethods)`, with
# `method_added` defined in `ClassMethods`.
#
# That second shape is not exotic. It is how Thor registers commands, and so
# `Bundler::CLI.commands` holds 3 of its ~30: the three that Thor's `register`
# adds directly. Every other one is registered by `method_added` firing after
# a `desc`, so `zeo bundle install` answers `Could not find command
# "install"` while every one of those methods is present on the class.
#
# `Bundler::CLI.instance_methods` lists add, binstubs, cache, check, clean,
# console, env and the rest. They exist. Thor just never heard about them.
#
# WHY IT IS NOT A ONE-LINE FIX: `runtime_meta::api::def_hook_runs` asks
# `class_method_owner` at run time and its comment says an extended module's
# copy is found -- and read on its own that is true. What does not happen is
# the ASK: a `def` the compiler placed statically does not go through the
# run-time definition path at all, so nothing consults the gate. Making every
# `def` consult it would cost the whole corpus; the honest fix is a
# program-wide "some `method_added` exists" gate the emitter can read, the
# same shape `global_def_hook` already has.
module Hook
  def self.included(base) = base.extend(ClassMethods)

  module ClassMethods
    def method_added(name)
      (@added ||= []) << name
    end

    def added = @added
  end
end

class Uses
  include Hook
  def a; end
  def b; end
end

p Uses.added

# The inherited spelling, which DOES work, so the two are side by side.
class Base
  def self.method_added(name) = ((@seen ||= []) << name)
  def self.seen = @seen
end

class Child < Base
  def one; end
end

p Child.seen

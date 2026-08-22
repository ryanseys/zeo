# A later `def self.x` reaches BACK: a call written above the reopen answers
# with the reopened body. The INSTANCE-method half of this is fixed (see
# `tests/a_later_def_on_a_module_installs_at_its_line.rb`); the class-method
# half is untouched, on a plain class and on a module alike.
#
# `analyze::redefs` is the mechanism and it skips class methods outright --
# `if *is_class_method { continue }` in both of its history scans, so a
# `(class, name)` pair is never even a candidate. What it would need is the
# same three pieces the instance side has, on the singleton channel:
#
#   * `positional_redefs` installs the FIRST body at boot. The class-method
#     twin is `runtime_define_singleton_method`, not `runtime_replace_method`;
#   * `runtime_patches` takes the name off the static route, and the
#     class-method call site reads a different set (`may_be_patched_at_runtime`
#     is asked of the instance name);
#   * a `HirNode::MethodRedefine` per later body needs a class-method flavour,
#     since its emitter arm installs on the object channel.
#
# `module_function` reaches this the same way, its class-method copy being an
# ordinary `def self.x` by the time analyze is done -- which is why the two
# shapes below answer identically.

class C
  def self.t = "t1"
end
p C.t
class C
  def self.t = "t2"
end
p C.t

module F
  module_function
  def g = "g1"
end
p F.g
module F
  module_function
  def g = "g2"
end
p F.g

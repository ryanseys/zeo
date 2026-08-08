# A `private` in a `class << self` body is a CURSOR that persists across a
# block: methods `define_method` installs after it are PRIVATE singleton
# methods. Oracle-verified -- `Vis.hidden` raises NoMethodError.
#
# zeo consumes a bare `private` in a class body as a COMPILE-TIME cursor
# (lower/defs.rs) and bakes it into the `def`s that follow. A runtime
# `define_method` is not a `def`, so nothing carries the cursor to it, and the
# method comes out public.
#
# The runtime half already exists: `runtime_define_method` takes a class-body
# frame and reads `f.vis`. What is missing is the cursor reaching the
# singleton's own class body, where the residual statement now runs.
class Vis
  class << self
    private
    [:hidden].each { |n| define_method(n) { :h } }
    public
    [:shown].each { |n| define_method(n) { :s } }
  end
end
p Vis.singleton_class.private_method_defined?(:hidden)
p Vis.singleton_class.public_method_defined?(:shown)
p (Vis.hidden rescue $!.class)
p Vis.shown

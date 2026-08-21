# A `class` written in a box snippet binds on the MAIN box's `Object`, so
# main can reach it by name. A box's whole point is that it cannot.
#
# `clif/stmt.rs`'s `eval_class_def` falls back to `OBJECT_CLASS` and never
# consults `fx.box_id`, so `zeo_rt_eval_class_open` opens the class on the
# shared root. zeo's top-level constants ARE box-isolated by a different
# mechanism -- `box_top()` routes a box's top-level constant onto the box's
# surrogate ClassId, so the `(owner, name)` key already separates boxes --
# and this path simply does not use it.
#
# `defined?` answers nil correctly, because that fold runs at compile time
# against a table the class was never registered in; only the READ escapes.
# The two disagreeing is the tell.
#
# The fix is G7's B0: the class header mints through the box's own root.
#
# Oracle: main cannot see the box's class, so the read raises NameError.
b = Ruby::Box.new
b.eval("class Escapee; def self.hi = 'in box'; end")
p defined?(Escapee)
begin
  p Escapee
rescue NameError => e
  p e.class
end

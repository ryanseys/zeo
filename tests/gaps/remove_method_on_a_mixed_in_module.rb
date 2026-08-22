# `remove_method` on a MODULE must take the name away from every class that
# mixed the module in. zeo keeps answering it there.
#
# The cause is the materialization: analyze copies a module's instance
# methods onto each includer at compile time, so `C.new.u` resolves through
# C's OWN row and never asks S2 at all. `remove_method` records a tombstone
# (`OverlayEntry::removed`) against the MODULE's id, which the walk honours
# at the module's own position -- a position the flattened lookup does not
# visit.
#
# Both verbs of the mixin are affected, and so is a `def` the module gains
# later (the reverse direction already works, so the two halves disagree).
#
# Pre-existing, and independent of the MRO occurrence pass -- it reproduces
# with a single `include` and with a single `prepend`. The fix shape: a
# removal on a module must invalidate the materialized copies its hosts
# carry, the way `runtime_replace_method` already reaches them.

module S2
  def u = "s2"
end
class C0
  include S2
end
p C0.new.u
S2.send(:remove_method, :u)
begin
  p C0.new.u
rescue NoMethodError => e
  p e.class.to_s
end

module S3
  def v = "s3"
end
class C2
  prepend S3
end
p C2.new.v
S3.send(:remove_method, :v)
begin
  p C2.new.v
rescue NoMethodError => e
  p e.class.to_s
end

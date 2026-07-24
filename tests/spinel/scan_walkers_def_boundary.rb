# Audit follow-up to #450 cascade 1. Several AST-walking helpers in
# spinel_analyze.rb recursively descend into nested DefNode /
# ClassNode / ModuleNode bodies without resetting scope context.
# That leaks per-method state (locals, param-method observations,
# push-element types) across method boundaries and pollutes the
# outer scope. cascade 1 of #450 was scan_locals itself; this file
# pins three siblings:
#
#   - scan_locals_first_type   (used by infer_lambda_param_types)
#   - scan_poly_assigns         (used by detect_poly_locals)
#   - collect_param_methods     (used by infer_cls_meth_param_from_body)
#   - collect_param_push_elem_types  (used by infer_param_array_type_from_body)
#   - scan_param_body_write_unify     (used by widen_param_types_from_body_writes)
#
# All of these now early-return at DefNode / ClassNode / ModuleNode.
#
# The shape: a top-level module with one def whose local shadows
# an outer name. Pre-fix, the outer scope's poly-detection / param
# inference would conflate the two and the resulting types would
# either widen unnecessarily or fail to widen for the right call
# site.

module Outer
  def self.process(path)
    # `path` is the param; body uses it as a string.
    parts = path.split("/")
    parts.length
  end
end

# Top-level local with the same name as Outer.process's param.
# Pre-fix, this main-level `path` would be conflated with Outer's
# param in scan_poly_assigns / scan_locals if scan walked into
# the module body.
path = "/tmp/db"
puts Outer.process(path)
puts Outer.process("/articles/1")

# Module with a deeper structure: two defs, each with the same
# param name, calling each other. The cascade-1 shape.
module Resolver
  def self.outer_resolve(path)
    inner_resolve(path)
  end

  def self.inner_resolve(path)
    path.upcase + ":" + path.length.to_s
  end
end

puts Resolver.outer_resolve("hello")
puts Resolver.outer_resolve("world")

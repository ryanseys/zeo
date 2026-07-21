# #512. Inside `module M` body, a bare method-name argument
# (`M.use(h)` where `h` should resolve to `M.h`) used to infer
# as `int` (the default for unresolved bare calls) and propagate
# that wrong type onto the called method's parameter. The
# downstream `opts.length` then dispatched on the int receiver
# and emitted 0.
#
# Fix: in infer_call_type's bare-call branch, when
# @current_lexical_scope names a module and the bare name
# matches a registered `<scope>_cls_<name>` cmeth in @meth_names,
# return that cmeth's recorded return type instead of falling
# through to the int default.
#
# The remaining (and orthogonal) `use(h)` shape — where the
# outer call is ALSO bare — still emits a warning; the bare-call
# argument propagation pass needs the same module-scope lookup
# to widen the callee's params from the call site's arg types.

module M
  def self.h
    {"" => ""}
  end
  def self.use(opts)
    opts.length
  end
  M.use(h)
end

puts M.use(M.h)

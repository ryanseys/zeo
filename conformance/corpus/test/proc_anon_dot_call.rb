# `proc { ... }.call` — the receiver is the inline `proc { ... }` call,
# not a local variable holding the proc. compile_dot_call_expr only
# matched LocalVariableReadNode receivers and emitted sp_proc_call
# against `lv_<name>`; the inline form fell through to
# warn_unresolved_call and the proc body was never invoked.
#
# Fix: after the LocalVariableReadNode special-case, fall through to
# a generic branch keyed on `infer_type(recv) == "proc"` that compiles
# the receiver expression and feeds it to sp_proc_call. The check is
# node-shape-agnostic — any receiver whose inferred type is `proc`
# routes through the same emitter.
#
# Coverage scope (all routed through `infer_type(recv) == "proc"`):
# - Anonymous proc literal `.call`           — CallNode receiver
# - Anonymous proc with args                 — CallNode receiver
# - Method returning a proc, immediately called (factory pattern)
# - Ternary returning a proc                 — IfNode receiver
#
# Out of scope (separate code paths / pre-existing limitations):
# - lambda { ... }.call / ->() { ... }.()
#     Lambdas have a separate handler (`compile_lambda_call_expr` at
#     ~line 11941 in master). Not exercised by this PR.
# - @ivar = proc { ... }; @ivar.call
#     Spinel's ivar codegen rejects assigning sp_Proc* to the ivar
#     slot today. Pre-existing inference/codegen gap on proc ivars,
#     independent of this PR.
# - $global = proc { ... }; $global.call
#     Spinel's $global proc inference is unverified. Separate concern.
# - [proc1, proc2].each(&:call)
#     Symbol#to_proc (`&:sym`) is unsupported in master (see
#     `spinel_codegen.rb:1067` TODO). Added in rs-no-recv-io-expr-bridge.

def show(s)
  puts s
end

# Anonymous proc literal — the original bug.
proc { show("anon") }.call
proc { show("second") }.call
proc { |n| show((n * 2).to_s) }.call(7)

# CallNode receiver returning a proc (factory pattern).
def factory
  proc { show("factory") }
end
factory.call

# Ternary returning a proc — both branches must unify to `proc`.
cond = true
(cond ? proc { show("ternary-true") } : proc { show("ternary-false") }).call

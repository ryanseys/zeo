# A snippet's compile registers nothing, so a `class << self; prepend M; end`
# inside an eval'd class body has no singleton-prepend splice to attach to.
# Compiling it anyway would drop the prepend silently; zeo refuses the snippet
# with a catchable `NotImplementedError` instead
# (`crates/zeo/src/eval.rs::scope_refusals`). This golden pins the refusal
# message; the sibling arms of `scope_refusals` (`MethodRedefine`, `DefHook`)
# are defensive -- analyze splices those nodes only in whole-program mode, so
# no snippet can reach them.
#
# --- ruby 4.0.6 answers ---
# decorated

# A snippet whose class body carries `class << self; prepend ...` is refused
# loudly: the singleton-prepend splice reads a decision only a whole-program
# compile makes, and a silently wrong answer is the alternative
# (`crates/zeo/src/eval.rs::scope_refusals`). The `.divergence` sidecar
# records ruby's answer.
module Deco
  def decorated = "decorated"
end

begin
  eval("class EvalPrepend; class << self; prepend Deco; end; end")
  puts EvalPrepend.decorated
rescue NotImplementedError => e
  puts e.message
end

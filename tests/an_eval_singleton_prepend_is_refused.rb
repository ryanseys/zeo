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

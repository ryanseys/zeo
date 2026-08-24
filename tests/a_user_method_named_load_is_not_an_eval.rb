# A program that defines its own `load`, `eval` or `require` used to carry the
# whole embedded run-time compiler -- 14 MB, and all 170 builtin class tables
# with it -- because the predicate that decides matched the call's NAME and
# nothing else.
#
# A RECEIVERLESS call resolves the way DISPATCH resolves it: a user method of
# that name in the enclosing class's chain is what runs, and `Kernel#eval` is
# unreachable from there. This pins that the narrowed answer agrees with what
# ruby actually calls -- if it did not, the wrong method would answer.
#
# An explicit receiver is deliberately NOT narrowed: `Binding#eval` is a real
# eval, and nothing static separates it from `obj.eval` on a user object.

def load(x) = [:top_level_load, x]
p load(1)

class K
  def eval(src) = [:k_eval, src]
  def go = eval("ignored")
end
p K.new.eval("x")
p K.new.go

module M
  def self.require(feature) = [:m_require, feature]
  def self.go = require("nothing")
end
p M.require("a")
p M.go

# Kernel's own still answers where no user method shadows it.
p eval("1 + 1")
p defined?(Kernel.instance_method(:eval))

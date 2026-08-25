# `__callee__` answers the name the method was CALLED by -- through an
# alias, the alias. The compiled top-level shape already agrees; a method
# DEFINED AT RUNTIME (a `def` inside a block) answers the original name
# through its alias. (Found by the 2026-08-24 probe sweep.)
def cal = [__method__, __callee__]
alias caz cal
p caz
[1].each do
  def dyn = __callee__
  alias dyz dyn
end
p dyz

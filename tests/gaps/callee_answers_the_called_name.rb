def cal = [__method__, __callee__]
alias caz cal
p caz
[1].each do
  def dyn = __callee__
  alias dyz dyn
end
p dyz

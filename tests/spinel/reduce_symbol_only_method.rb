# `reduce(:sym)` / `inject(seed, :sym)` names the method by Symbol, which is not
# a CallNode, so the reachability walk never saw the name and pruned the method.
# The fold then fell through to the unresolved-call raise -- "undefined method
# '^' for an instance of F" for a method the program plainly defines. Only a
# method with NO other call site was affected, which is what made it look like
# an operator problem rather than a reachability one (#4069).
class F
  attr_reader :u
  def initialize(u) = @u = u
  def ^(other) = F.new(u + other.u)
  def to_s = u.to_s
end
p [F.new(1), F.new(2)].reduce(F.new(0), :^).to_s

class G
  attr_reader :n
  def initialize(n) = @n = n
  def +(o) = G.new(n + o.n)
  def merge_with(o) = G.new(n * o.n)
end
p [G.new(1), G.new(2)].reduce(:+).n
p [G.new(1), G.new(2)].inject(G.new(10), :+).n
p [G.new(2), G.new(3)].reduce(:merge_with).n
p [G.new(2), G.new(3)].inject(G.new(4), :merge_with).n

# a method that also has an ordinary call site still works
class H
  attr_reader :v
  def initialize(v) = @v = v
  def |(o) = H.new(v + o.v)
end
p (H.new(1) | H.new(2)).v
p [H.new(1), H.new(2)].reduce(H.new(0), :|).v

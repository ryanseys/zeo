module M004
  def double004(n); n * 2; end
  def greet004; "hi"; end
end
include M004
p double004(21)
p greet004

module M005
  def self.triple005(n); n * 3; end
end
include M005
p M005.triple005(4)

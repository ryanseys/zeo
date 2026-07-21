# A constant aliasing a class (`A = M::N`) works as a method-call receiver:
# the alias receiver is rewritten to the underlying class so `A.foo`
# dispatches like the direct `M::N.foo`.

module M
  class N
    def self.foo; 42; end
    def self.bar(x); x + 1; end
  end
end

A = M::N
puts A.foo          # 42
puts A.bar(10)      # 11

class Top
  def self.greet; "hi"; end
end
B = Top
puts B.greet        # hi
puts "done"

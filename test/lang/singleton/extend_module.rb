# `class C; extend M; end` exposes M's instance methods as class methods on
# C, so `C.<m>` reaches the module's body.

module M
  def ext_method; "extended"; end
  def hello(name); "hi " + name; end
end

class C
  extend M
end

puts C.ext_method
puts C.hello("world")

# `def self.X` on M stays on M and is NOT pulled into the extending
# class (CRuby semantics).
module N
  def self.solo; "solo-on-N"; end
  def shared; "shared"; end
end

class D
  extend N
end

puts D.shared
puts N.solo
__END__
extended
hi world
shared
solo-on-N

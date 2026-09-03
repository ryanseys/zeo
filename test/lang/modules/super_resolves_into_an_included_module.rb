# A compile-time scan of the SUPERCLASS chain alone misses a method that
# only an included module defines; the runtime walk covers the real
# ancestry, so both a class->module and a module->module `super` land.

module Greet
  def hi
    "[hi]"
  end
end
class C
  include Greet
  def hi
    super
  end
end
module M1
  def tag
    "M1"
  end
end
module M2
  include M1
  def tag
    "M2(#{super})"
  end
end
class F
  include M2
  def tag
    "F[#{super}]"
  end
end
puts C.new.hi
puts F.new.tag
__END__
[hi]
F[M2(M1)]

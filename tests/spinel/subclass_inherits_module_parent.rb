# `class Sub < Base` inside `module M` used to record `cls_parents`
# as bare "Base" rather than "M_Base", so emit_class_fields' lookup
# of the parent missed and Sub came out as an empty struct — every
# field inherited from Base was dropped on the C side.

module M
  class Base
    def initialize(x)
      @x = x
    end
  end

  class Sub < Base
    def double
      @x * 2
    end
    def x_plus(n)
      @x + n
    end
  end

  class Plain
    def initialize
      @y = 99
    end
  end

  class PlainSub < Plain
    def boost
      @y + 1
    end
  end
end

s = M::Sub.new(7)
puts s.double
puts s.x_plus(100)

ps = M::PlainSub.new
puts ps.boost

# When a class lives inside a module and another class in the same
# module is referenced via a bare name from one of its method bodies,
# the inferred type of the resulting ivar/local must carry the same
# namespace prefix that the class is registered under. Otherwise
# `c_type` falls back to the short name and the C code references an
# undeclared `sp_Foo *` that does not match the `sp_M_Foo *` typedef.
#
# Pre-fix, `Inner.new` inside `Outer#initialize` was scanned with no
# lexical scope set on the compiler, so `Inner` resolved to bare and
# the ivar got recorded as `obj_Inner` while the actual class is
# registered under `sp_M_Inner`. The struct field declaration would
# then read `sp_Inner * iv_inner` and the C compiler would reject it
# with `unknown type name 'sp_Inner'`.

module M
  class Inner
    def initialize(x)
      @x = x
    end
    attr_reader :x
  end

  class Outer
    def initialize
      @inner = Inner.new(10)
    end
    def via_ivar
      @inner.x
    end
    def via_local
      tmp = Inner.new(20)
      tmp.x
    end
  end
end

puts M::Outer.new.via_ivar
puts M::Outer.new.via_local

# Verified against real Ruby first: a subclass's own `@@x = ...`
# does NOT shadow -- it finds and mutates the SAME storage inherited
# from the superclass (real Ruby's actual, if slightly surprising,
# class-variable semantics). This is also the exact scenario zeo's
# own C implementation gets wrong (a subclass writing a superclass-only
# cvar allocates fresh, separate storage there -- an outright compile
# failure in zeo's case).

class Base
  @@x = 1
  def base_x
    @@x
  end
end
class Sub < Base
  @@x = 99
  def sub_x
    @@x
  end
end
puts Base.new.base_x
puts Sub.new.sub_x
__END__
99
99

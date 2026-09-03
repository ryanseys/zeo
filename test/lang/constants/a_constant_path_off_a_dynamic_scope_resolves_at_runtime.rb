# `expr::NAME` where the scope is a runtime value (`self.class::Reason`,
# optparse) resolves the constant off that value at runtime, walking its
# ancestry -- so a subclass sees its own override.

class Base
  Reason = "base"
  def reason; self.class::Reason; end
end
class Sub < Base
  Reason = "sub"
end
puts Base.new.reason
puts Sub.new.reason
holder = Base
puts holder::Reason
__END__
base
sub
base

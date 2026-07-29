# __method__ returns the name the method was DEFINED with, __callee__ the name
# it was CALLED through -- the two diverge for an aliased method.
class CalleeTest
  def original_name
    [__method__, __callee__]
  end
  alias aliased_name original_name
end
p CalleeTest.new.aliased_name

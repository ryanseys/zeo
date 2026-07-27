# __method__ should return the name the method was DEFINED with, while
# __callee__ returns the name it was CALLED through -- they diverge for an
# aliased method. zeo returns the call-name for both instead of tracking the
# original definition name for __method__.
class CalleeTest
  def original_name
    [__method__, __callee__]
  end
  alias aliased_name original_name
end
p CalleeTest.new.aliased_name

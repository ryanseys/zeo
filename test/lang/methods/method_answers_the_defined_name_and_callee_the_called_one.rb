# The two diverge for an aliased method: `__method__` is the original, and
# `__callee__` is the alias.
class CalleeTest
  def original_name
    [__method__, __callee__]
  end
  alias aliased_name original_name
end
p CalleeTest.new.aliased_name
__END__
[:original_name, :aliased_name]

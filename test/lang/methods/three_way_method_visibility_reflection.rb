# Private/public/protected are tracked distinctly: `instance_methods`
# keeps public+protected, the prefixed queries match exactly one
# visibility, `respond_to?`'s default skips private AND protected, and a
# subclass may re-declare an inherited method's visibility.

class Account
  def deposit; end
  private
  def log; end
  protected
  def compare; end
end
def s(a); a.map(&:to_s).sort; end
p s(Account.instance_methods(false))
p s(Account.public_instance_methods(false))
p s(Account.protected_instance_methods(false))
p s(Account.private_instance_methods(false))
p Account.public_method_defined?(:deposit)
p Account.protected_method_defined?(:compare)
p Account.private_method_defined?(:log)
p Account.public_method_defined?(:compare)
a = Account.new
p a.respond_to?(:compare)
p a.respond_to?(:compare, true)
class Base
  def m; end
end
class Sub < Base
  private :m
end
p Sub.new.respond_to?(:m)
p Sub.private_method_defined?(:m)
__END__
["compare", "deposit"]
["deposit"]
["compare"]
["log"]
true
true
true
false
false
true
false
true

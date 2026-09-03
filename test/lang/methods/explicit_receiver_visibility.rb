# Ruby decides visibility from the SHAPE of the call, not from the value the
# receiver holds. `vm_call_method` runs no check at all for an implicit
# receiver, for the literal `self` keyword, or for `send` (all FCALL); every
# other explicit receiver is checked.
#
# zeo enforced that only where the receiver's class was known at compile time.
# A dynamic call skipped it entirely, so `1.puts("x")` printed, `"ab".initialize`
# answered nil, and a private method reached through a local that merely HELD
# self went through.

def try(label)
  v = yield
  puts "#{label} => #{v.inspect}"
rescue NoMethodError => e
  puts "#{label} !! #{e.class}: #{e.message}"
end

class R
  def on_self = self.helper
  def bare = helper

  # A local holding `self` is an ordinary explicit receiver. Only the `self`
  # KEYWORD is exempt, which is why this one raises and `on_self` does not.
  def through_local
    me = self
    me.helper
  end

  def on_other(o) = o.helper
  private def helper = :helped
end

try("R.new.on_self") { R.new.on_self }
try("R.new.bare") { R.new.bare }
try("R.new.through_local") { R.new.through_local }
try("R.new.on_other(R.new)") { R.new.on_other(R.new) }
try("R.new.helper") { R.new.helper }
try("R.new.send(:helper)") { R.new.send(:helper) }
try("R.new.__send__(:helper)") { R.new.__send__(:helper) }
try("R.new.public_send(:helper)") { R.new.public_send(:helper) }
try("R.new.respond_to?(:helper)") { R.new.respond_to?(:helper) }
try("R.new.respond_to?(:helper, true)") { R.new.respond_to?(:helper, true) }

# `protected` is the receiver-sensitive one: reachable when the CALLER's self
# is a kind of the method's owner, whoever the receiver is.
class Money
  def >(other) = amount > other.amount
  protected def amount = 7
end
class Cash < Money; end
class Outsider
  def peek(m) = m.amount
end

try("Money.new > Money.new") { Money.new > Money.new }
try("Money.new > Cash.new") { Money.new > Cash.new }
try("Cash.new > Money.new") { Cash.new > Money.new }
try("Outsider.new.peek(Money.new)") { Outsider.new.peek(Money.new) }
try("Money.new.amount") { Money.new.amount }
try("Money.new.send(:amount)") { Money.new.send(:amount) }
try("Money.new.public_send(:amount)") { Money.new.public_send(:amount) }

# The builtin rows answer the same way -- Kernel's methods are private, on
# every receiver, whatever its class.
try("1.puts") { 1.puts("x") }
try("'ab'.puts") { "ab".puts("x") }
try("nil.puts") { nil.puts("x") }
try("[].raise") { [].raise("boom") }
try("'ab'.initialize") { "ab".initialize }
try("[].initialize") { [].initialize }
try("Object.new.initialize") { Object.new.initialize }
try("RuntimeError.new.initialize") { RuntimeError.new.initialize }
try("3.7.require") { 3.7.require("set") }

# A class method the class actually provides is the one that answers, so a
# same-named private Kernel method further down cannot reach past it. `open`
# is the case that matters: `File.open` is File's own public singleton method,
# while `Kernel#open` is private.
try("File.respond_to?(:open)") { File.respond_to?(:open) }
try("String.name") { String.name }
try("Integer.instance_methods.class") { Integer.instance_methods.class }

# A private CLASS method is checked the same way.
class Factory
  def self.build = :built
  def self.make_via_self = self.build
  private_class_method :build
end
try("Factory.make_via_self") { Factory.make_via_self }
try("Factory.build") { Factory.build }
try("Factory.send(:build)") { Factory.send(:build) }

# Visibility changed after the fact is a RUNTIME property, so a site that
# dispatches dynamically has to see the change. (A site whose receiver class is
# known at compile time answers from the compile-time table instead -- see
# tests/gaps/runtime_private_on_typed_receiver.rb.)
class Late
  def open_now = :open
end
try("Late#open_now before") { Late.new.open_now }
Late.send(:private, :open_now)
try("Late#open_now after") { [Late.new].first.open_now }
__END__
R.new.on_self => :helped
R.new.bare => :helped
R.new.through_local !! NoMethodError: private method 'helper' called for an instance of R
R.new.on_other(R.new) !! NoMethodError: private method 'helper' called for an instance of R
R.new.helper !! NoMethodError: private method 'helper' called for an instance of R
R.new.send(:helper) => :helped
R.new.__send__(:helper) => :helped
R.new.public_send(:helper) !! NoMethodError: private method 'helper' called for an instance of R
R.new.respond_to?(:helper) => false
R.new.respond_to?(:helper, true) => true
Money.new > Money.new => false
Money.new > Cash.new => false
Cash.new > Money.new => false
Outsider.new.peek(Money.new) !! NoMethodError: protected method 'amount' called for an instance of Money
Money.new.amount !! NoMethodError: protected method 'amount' called for an instance of Money
Money.new.send(:amount) => 7
Money.new.public_send(:amount) !! NoMethodError: protected method 'amount' called for an instance of Money
1.puts !! NoMethodError: private method 'puts' called for an instance of Integer
'ab'.puts !! NoMethodError: private method 'puts' called for an instance of String
nil.puts !! NoMethodError: private method 'puts' called for nil
[].raise !! NoMethodError: private method 'raise' called for an instance of Array
'ab'.initialize !! NoMethodError: private method 'initialize' called for an instance of String
[].initialize !! NoMethodError: private method 'initialize' called for an instance of Array
Object.new.initialize !! NoMethodError: private method 'initialize' called for an instance of Object
RuntimeError.new.initialize !! NoMethodError: private method 'initialize' called for an instance of RuntimeError
3.7.require !! NoMethodError: private method 'require' called for an instance of Float
File.respond_to?(:open) => true
String.name => "String"
Integer.instance_methods.class => Array
Factory.make_via_self => :built
Factory.build !! NoMethodError: private method 'build' called for class Factory
Factory.send(:build) => :built
Late#open_now before => :open
Late#open_now after !! NoMethodError: private method 'open_now' called for an instance of Late

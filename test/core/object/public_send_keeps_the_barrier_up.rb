# `send`/`__send__` are deliberately visibility-blind. `public_send` is not:
# ruby resolves the target and refuses a non-public one with a rescuable
# NoMethodError, with NONE of the relaxations an ordinary explicit-receiver
# call gets -- no literal-`self` exemption, no protected relatedness.

class Acct
  def initialize(n) = @n = n
  def pub = :pub
  def compare(other) = other.public_send(:guarded)
  def own = public_send(:secret)
  protected def guarded = :prot
  private def secret = :shh
end

def try
  yield
rescue NoMethodError => e
  [e.class, e.message]
end

a = Acct.new(1)
p a.public_send(:pub)
p a.send(:secret)
p a.__send__(:secret)
p(try { a.public_send(:secret) })
p(try { a.public_send(:guarded) })

# A name computed at run time takes the same route.
n = "secret"
p(try { a.public_send(n) })
p a.send(n)

# A receiverless `public_send` still enforces it: ruby checks the RESOLVED
# entry with a public scope, which has nothing to do with whether the site
# wrote a receiver.
p(try { a.own })

# ...and protected relatedness does NOT relax it, even between two instances
# of the same class.
p(try { a.compare(Acct.new(2)) })

# A public method reached through either spelling is just a call.
p a.public_send(:pub)
p a.send(:pub)

# No name at all.
p(begin
  a.public_send
rescue ArgumentError => e
  [e.class, e.message]
end)
__END__
:pub
:shh
:shh
[NoMethodError, "private method 'secret' called for an instance of Acct"]
[NoMethodError, "protected method 'guarded' called for an instance of Acct"]
[NoMethodError, "private method 'secret' called for an instance of Acct"]
:shh
[NoMethodError, "private method 'secret' called for an instance of Acct"]
[NoMethodError, "protected method 'guarded' called for an instance of Acct"]
:pub
:pub
[ArgumentError, "no method name given"]

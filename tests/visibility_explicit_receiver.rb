# Ruby's THIRD call mode: an explicit receiver enforces visibility, where a
# receiverless call does not. A `protected` method is reachable only when the
# CALLER'S SELF is kind_of the method's owner.
#
# The class-method case is the asymmetry that was wrong: in `def self.peek(a);
# a.balance; end`, `self` is the class object, and a class object is no kind
# of the class whose instances hold `balance`. zeo measured the caller by its
# lexical class instead, so the class method reached a protected method ruby
# refuses.

def t; yield; rescue NoMethodError => e; puts "NoMethodError: #{e.message[0, 44]}"; end

class A
  def pub; :pub; end
  private def priv; :priv; end
  protected def prot; :prot; end
  def sec; :sec; end
  private :sec
  def call_priv; priv; end
  def call_self_priv; self.priv; end
  def cmp(o); o.prot; end
end
a = A.new
t { p a.pub }
t { p a.priv }
t { p a.prot }
t { p a.sec }
t { p a.call_priv }
t { p a.call_self_priv }
t { p a.cmp(A.new) }
t { p a.send(:priv) }
t { p a.public_send(:priv) }
t { p a.respond_to?(:priv) }
t { p a.respond_to?(:priv, true) }

class Account
  def initialize(b); @balance = b; end
  def >(other); balance > other.balance; end
  def bigger_than_all?(others); others.all? { |o| balance > o.balance } end
  def self.peek(a); a.balance; end
  def peek_self; self.balance; end
  def peek_new; Account.new(0).secret; end
  def to_s; "acct:#{secret}"; end
  protected
  def balance; @balance; end
  private
  def secret; 42; end
  public
  def open; :open; end
  alias_method :hidden, :secret
end
class Savings < Account
  def cmp(o); balance > o.balance; end
  def poke(o); o.secret; end
end
acct = Account.new(10); s = Savings.new(5)
t { p acct > s }
t { p acct.bigger_than_all?([s, Account.new(1)]) }
t { p Account.peek(acct) }
t { p acct.peek_self }
t { p acct.peek_new }
t { p acct.balance }
t { p acct.secret }
t { p acct.open }
t { p acct.hidden }
t { p s.cmp(acct) }
t { p s.poke(acct) }
t { p acct.to_s }
t { p s.balance }

begin
  acct.secret
rescue NoMethodError => e
  p [e.name, e.receiver.class]
end
class Cfg
  attr_writer :w
  private :w=
  def set(v); self.w = v; end
end
cfg = Cfg.new
t { cfg.w = 6 }
t { p(cfg.w = 7) }
p cfg.set(8)
module Hidden
  private def mpriv; :mpriv; end
  protected def mprot; :mprot; end
  def via_self; mpriv; end
  def peer(o); o.mprot; end
end
class Host; include Hidden; end
h = Host.new
t { p h.mpriv }
t { p h.mprot }
t { p h.via_self }
t { p h.peer(Host.new) }

# The class-method asymmetry, stated on its own.
class Vault
  def initialize(n) = @n = n
  def self.peek(v) = v.amount
  def self.peek_send(v) = v.send(:amount)
  def compare(other) = amount <=> other.amount
  protected
  def amount = @n
end
module Helper
  def self.peek(v) = v.amount
end
class SubVault < Vault; end

t { p Vault.peek(Vault.new(1)) }
t { p Vault.peek_send(Vault.new(1)) }
t { p Helper.peek(Vault.new(1)) }
t { p Vault.new(2).compare(Vault.new(1)) }
t { p SubVault.new(2).compare(Vault.new(1)) }
t { p Vault.new(2).compare(SubVault.new(1)) }
t { p Vault.new(1).amount }
class << Vault
  def peek_singleton(v) = v.amount
end
t { p Vault.peek_singleton(Vault.new(1)) }

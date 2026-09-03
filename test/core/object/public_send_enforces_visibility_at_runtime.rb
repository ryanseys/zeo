# `public_send` is stricter than an ordinary explicit-receiver call: it
# rejects private AND protected, with no self-relatedness relaxation,
# because CRuby implements it by passing `Qundef` as the caller's self
# (vm_eval.c:1230) -- a sentinel nothing can be a kind of. This is a
# RUNTIME check (rb_method_call_status), so the program must compile and
# the NoMethodError must be rescuable. Covers explicit-receiver, implicit
# self, and a runtime-computed method name.

def err
  yield
rescue NoMethodError => e
  "#{e.class}: #{e.message}"
end
class Acct
  def balance = 100
  def peer_check(other) = other.guarded
  protected
  def guarded = "prot"
  private
  def secret = 42
end
class Inner
  def run = public_send(:hidden)
  private
  def hidden = 1
end
def dyn(o, m) = o.public_send(m)
a = Acct.new
p a.public_send(:balance)
puts err { a.public_send(:secret) }
puts err { a.public_send(:guarded) }
p a.send(:secret)
p a.peer_check(Acct.new)
puts err { Inner.new.run }
puts err { dyn(a, :secret) }
__END__
100
NoMethodError: private method 'secret' called for an instance of Acct
NoMethodError: protected method 'guarded' called for an instance of Acct
42
"prot"
NoMethodError: private method 'hidden' called for an instance of Inner
NoMethodError: private method 'secret' called for an instance of Acct

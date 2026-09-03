# `send` is not magic: it is an ordinary method on Kernel, so a class that
# defines its own wins the lookup by sitting earlier in the ancestor chain --
# and only Kernel's reinterprets its first argument as a method name.
#
# The interesting case is a receiver whose class the compiler cannot see. A
# `send` there has to ask the chain at RUNTIME rather than assume Kernel's.

require "socket"

# --- a user class that defines `send` ----------------------------------------
class Radio
  def send(message, urgency = 0) = "#{message}/#{urgency}"
  def beep = "beep"
end

# Through a typed local, and through one the compiler can't type.
r = Radio.new
p r.send("hi")
p r.send("hi", 2)
untyped, = [Radio.new]
p untyped.send("hi", 3)
p [Radio.new].first.send("bye")

# A subclass inherits the shadow.
class Walkie < Radio; end
w, = [Walkie.new]
p w.send("over")

# --- BasicSocket#send: the same rule, from a builtin -------------------------
# `a, b = UNIXSocket.pair` gives locals with no static type at all.
a, b = UNIXSocket.pair
p a.send("hello", 0)
p b.recv(5)
sock = a
p sock.send("bye", 0)
p b.recv(3)
a.close
b.close

# --- Kernel#send still reinterprets, for everything that does NOT shadow -----
class Plain
  def greet(who) = "hi #{who}"
  private def secret = "shh"
end
plain = Plain.new
p plain.send(:greet, "ada")
p plain.send("greet", "grace")
p plain.send(:secret)
mystery, = [Plain.new]
p mystery.send(:greet, "ada")
p mystery.send("secret")

# Builtin receivers take the same path.
p [3, 1, 2].send(:sort)
p "abc".send(:upcase)
p 5.send(:+, 6)
list, = [[3, 1, 2]]
p list.send(:sort)
p list.send(:push, 9)

# A block rides along.
p [1, 2, 3].send(:map) { |x| x * 2 }
nums, = [[1, 2, 3]]
p nums.send(:select) { |x| x.odd? }

# --- public_send is visibility-gated, shadow or not --------------------------
p plain.public_send(:greet, "ada")
begin
  plain.public_send(:secret)
rescue NoMethodError => e
  puts e.message
end
begin
  mystery.public_send(:secret)
rescue NoMethodError => e
  puts e.message
end

# A class defining `send` does NOT thereby define `public_send`.
p r.public_send(:beep)
p untyped.public_send(:beep)

# --- __send__ always resolves through the chain ------------------------------
p plain.__send__(:greet, "ada")
p mystery.__send__(:greet, "grace")
p r.__send__(:beep)
__END__
"hi/0"
"hi/2"
"hi/3"
"bye/0"
"over/0"
5
"hello"
3
"bye"
"hi ada"
"hi grace"
"shh"
"hi ada"
"shh"
[1, 2, 3]
"ABC"
11
[1, 2, 3]
[3, 1, 2, 9]
[2, 4, 6]
[1, 3]
"hi ada"
private method 'secret' called for an instance of Plain
private method 'secret' called for an instance of Plain
"beep"
"beep"
"hi ada"
"hi grace"
"beep"

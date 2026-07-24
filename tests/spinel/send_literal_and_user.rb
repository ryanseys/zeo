# A literal-name send/public_send is rewritten to a direct call, so it keeps
# working. A class that defines its own `send` is dispatched normally and is
# never mistaken for Kernel#send (even with a runtime argument), so the
# runtime-name diagnostic must not fire for it.
class Calc
  def double(n)
    n * 2
  end

  def label
    "calc"
  end
end

class Mailer
  def send(msg)
    "sent: #{msg}"
  end
end

c = Calc.new
puts c.send(:double, 5)        # literal symbol name -> direct call
puts c.public_send("label")    # literal string name -> direct call

m = Mailer.new
greeting = "hi"
puts m.send(greeting)          # user-defined Mailer#send, runtime arg
puts m.send(greeting + "!")    # still the user method, not Kernel#send

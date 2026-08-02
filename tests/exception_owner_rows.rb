# The four rows ruby declares ON an exception class that zeo used to answer
# from an ancestor. Reflection has to name the same owner, because a rescue
# handler that asks `SignalException.instance_methods(false)` gets a different
# answer from `Exception`'s.
#
# `Exception#respond_to?` is the odd one: it gives Kernel's answer exactly, and
# exists only because `Exception` also carries a private `method_missing`.

p Exception.instance_methods(false).sort
p SignalException.instance_methods(false).sort
p UncaughtThrowError.instance_methods(false).sort

# Each name reports the class that declares it, not the one that answers it.
%i[respond_to? message inspect].each { |m| p Exception.instance_method(m).owner }
%i[signm signo].each { |m| p SignalException.instance_method(m).owner }
%i[to_s tag value].each { |m| p UncaughtThrowError.instance_method(m).owner }

# A subclass declares none of them, and still answers all of them.
class Custom < SignalException; end
p Custom.instance_methods(false)
p Custom.new("TERM").signo
p Custom.new("TERM").signm

# `respond_to?` keeps the whole protocol -- private opt-in, the
# `respond_to_missing?` hook, and the arity check.
class Hooked < StandardError
  def respond_to_missing?(name, _include_private) = name == :magic
end
e = Hooked.new("boom")
p [e.respond_to?(:message), e.respond_to?(:nope), e.respond_to?(:magic)]
p [e.respond_to?(:initialize), e.respond_to?(:initialize, true)]
p e.respond_to?("message")
begin
  e.respond_to?
rescue ArgumentError => err
  p err.message
end
begin
  e.respond_to?(1)
rescue TypeError => err
  p err.message
end

# The signal pair reads through a `message` override, because `signm` sends.
class Loud < SignalException
  def message = "LOUD"
end
p Loud.new("TERM").signm

# `UncaughtThrowError#to_s` renders the tag, which `Exception#to_s` cannot.
begin
  throw :nope, 42
rescue UncaughtThrowError => err
  p err.to_s
  p [err.tag, err.value]
end

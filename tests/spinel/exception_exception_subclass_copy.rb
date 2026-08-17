class SimpleError < StandardError
  attr_reader :code
  def initialize(code); @code = code; super("simple"); end
end
e010 = SimpleError.new(3); f010 = e010.exception("other"); p f010.class; p f010.message

e = SimpleError.new(7)
f = e.exception("changed")
p f.code
p f.equal?(e)
p e.exception.equal?(e)
r = (raise SimpleError.new(9) rescue $!.exception("x")); p [r.class, r.message]

class SubX < StandardError; end
p(SubX.new("t") == StandardError.new("t"))

e001 = SubX.new(3); f001 = e001.exception("other"); p f001.class; p f001.message

p(SubX.new("t") == SubX.new("t"))    # => true
p(SubX.new("m").exception.class)     # => SubX
p(SubX.new("m").cause)               # => nil
begin; raise SubX, "m"; rescue SubX => e002; p e002.cause; end   # => nil

# An exception subclass that defines its own to_s answers whatever that method
# answers -- a Symbol, say. The call went through the message helper instead,
# whose result is a string, and the Symbol came back empty.
class TS < StandardError
  def to_s; :sym; end
end
p(TS.new.to_s)

class TT < StandardError
  def to_s; "custom"; end
end
p(TT.new.to_s)
begin
  raise TT
rescue => e
  p e.to_s
end

class TU < StandardError; end
p(TU.new("m").message)
p(TU.new("m").to_s)
p(TU.new.message)

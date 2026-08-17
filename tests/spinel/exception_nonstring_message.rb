class TS < StandardError
  def to_s; :sym; end
end
p(TS.new.message)

r001 = (raise TS rescue $!.message); p r001   # Ruby: :sym   Spinel: "TS"

p(TS.new.to_s)   # => :sym

class TI < StandardError
  def to_s; 42; end
end
r = (raise TI rescue $!.message); p r
r = (raise TI rescue $!.to_s); p r
r = (raise "plain" rescue $!.message); p r
class TM < StandardError
  def message; :mm; end
end
r = (raise TM rescue $!.message); p r

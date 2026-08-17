class TS < StandardError
  def to_s; :sym; end
end
p(TS.new.message)
class TS2 < StandardError
  def to_s; "text"; end
end
p(TS2.new.message)
p(StandardError.new("plain").message)

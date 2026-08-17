r001 = (begin
  begin
    raise ArgumentError, "orig"
  ensure
    raise TypeError, "in-ensure"
  end
rescue => e001
  [e001.class.to_s, e001.cause.class.to_s]
end); p r001

r002 = (begin
  begin
    begin; raise ArgumentError, "a"; rescue; raise TypeError, "b"; end
  rescue
    raise
  end
rescue => e002
  [e002.class.to_s, e002.cause.class.to_s]
end); p r002
r3 = (begin; begin; raise "x"; rescue => e; raise ArgumentError, "y"; end; rescue => f; [f.class.to_s, f.cause.class.to_s]; end); p r3
r4 = (begin; raise "z"; rescue => g; g.cause.inspect; end); p r4

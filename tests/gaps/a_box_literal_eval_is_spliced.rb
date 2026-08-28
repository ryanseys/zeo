b = Ruby::Box.new
p b.eval("__FILE__")
begin
  b.eval("raise 'x'")
rescue RuntimeError => e
  p e.backtrace.first(2)
end

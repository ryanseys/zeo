b = binding
p eval("__FILE__", b, "fake.rb", 100)
p eval("__LINE__", b, "fake.rb", 100)
begin
  eval("raise 'x'", b, "fake.rb", 10)
rescue => e
  p e.backtrace.first
end

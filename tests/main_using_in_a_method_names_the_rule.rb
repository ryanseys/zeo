# `using` called from a method body raises RuntimeError "main.using is
# permitted only at toplevel"; zeo answers NoMethodError instead. (Found
# by the 2026-08-24 probe sweep.)
def use_it
  using Module.new
end
begin
  use_it
rescue Exception => e
  puts "#{e.class}: #{e.message}"
end

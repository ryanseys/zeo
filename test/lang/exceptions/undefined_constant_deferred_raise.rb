# An undefined constant used in class position raises a runtime NameError,
# never a compile-time failure -- shown across a pattern, a scoped constant
# write, and `raise Const, msg`.

x = 5
begin
  case x
  in NopeClass then puts "unreachable"
  else puts "unreachable"
  end
rescue => e
  puts "#{e.class}: #{e.message}"
end

# The scope is resolved BEFORE the RHS runs, so "rhs" never prints.
begin
  Nope::X = (puts "rhs"; 5)
rescue => e
  puts "#{e.class}: #{e.message}"
end

begin
  raise Nope, "msg"
rescue => e
  puts "#{e.class}: #{e.message}"
end
__END__
NameError: uninitialized constant NopeClass
NameError: uninitialized constant Nope
NameError: uninitialized constant Nope

# CRuby resolves the scope BEFORE evaluating the value, so the RHS side
# effect never runs.

r = begin
  Nope::X = (puts "rhs-ran"; 5)
  "assigned"
rescue => e
  "#{e.class}: #{e.message}"
end
puts r
__END__
NameError: uninitialized constant Nope

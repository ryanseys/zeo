class Flag; end
f = Flag.new
puts (f && 1).inspect
puts (nil || Flag.new).class
puts f ? "truthy" : "falsy"
puts (!f).inspect
__END__
1
Flag
truthy
false

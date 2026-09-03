# A raised native exception exposes NO `@message` ivar (CRuby stores it in a
# hidden slot): `instance_variables` is empty and `@message` reads nil.

begin
  raise ArgumentError, "boom"
rescue => e
  p e.instance_variables
  p e.instance_variable_get(:@message)
  puts e.message
end
__END__
[]
nil
boom

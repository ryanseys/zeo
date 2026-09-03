# `const_set` validates the constant name ("wrong constant name lower");
# zeo sets the lowercase constant. `const_get` already validates. And
# `remove_const` on a missing name prints the RECEIVER in its NameError
# ("constant #<Module:0x...>::Nope not defined"); zeo names only the
# constant. (Found by the 2026-08-24 probe sweep.)
begin
  Module.new.const_set(:lower, 1)
  puts "set succeeded"
rescue NameError => e
  puts e.message
end
begin
  Module.new.send(:remove_const, :Nope)
rescue NameError => e
  puts e.message.sub(/0x[0-9a-f]+/, "0xN")
end
__END__
wrong constant name lower
constant #<Module:0xN>::Nope not defined

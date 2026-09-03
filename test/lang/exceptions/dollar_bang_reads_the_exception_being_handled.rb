# `$!` is the current exception inside a rescue (block or modifier form),
# nil outside one.

p $!
r = (Integer("x") rescue $!.class)
p r
begin
  raise "boom"
rescue
  puts $!.message
end
p $!
__END__
nil
ArgumentError
boom
nil

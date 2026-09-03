p Array.instance_methods(false).include?(:chain)
p Array.instance_methods(false).include?(:member?)
p Array.instance_method(:chain).owner
p Array.instance_method(:member?).owner
__END__
false
false
Enumerable
Enumerable

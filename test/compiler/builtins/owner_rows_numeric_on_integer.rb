p Integer.instance_methods(false).include?(:+@)
p Integer.instance_methods(false).include?(:i)
p Integer.instance_methods(false).include?(:finite?)
p Integer.instance_method(:+@).owner
p Float.instance_methods(false).include?(:div)
p Float.instance_method(:div).owner
__END__
false
false
false
Numeric
false
Numeric

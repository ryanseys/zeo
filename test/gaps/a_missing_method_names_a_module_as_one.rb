# The NameError for a method a MODULE does not have.
begin
  Kernel.instance_method(:nope)
rescue NameError => e
  puts e.message
end
begin
  Comparable.instance_method(:nope)
rescue NameError => e
  puts e.message
end
begin
  String.instance_method(:nope)
rescue NameError => e
  puts e.message
end
__END__
undefined method 'nope' for module 'Kernel'
undefined method 'nope' for module 'Comparable'
undefined method 'nope' for class 'String'

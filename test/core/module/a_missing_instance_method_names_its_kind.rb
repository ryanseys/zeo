# `instance_method` reports the kind it was asked about: a module says
# module, a class says class.
[Kernel, Comparable, Enumerable, String, Class, Struct].each do |m|
  m.instance_method(:nope)
rescue NameError => e
  puts e.message
end
M = Module.new
begin
  M.instance_method(:nope)
rescue NameError => e
  puts e.message.sub(/'#<Module:0x\h+>'/, "'<anonymous>'")
end
__END__
undefined method 'nope' for module 'Kernel'
undefined method 'nope' for module 'Comparable'
undefined method 'nope' for module 'Enumerable'
undefined method 'nope' for class 'String'
undefined method 'nope' for class 'Class'
undefined method 'nope' for class 'Struct'
undefined method 'nope' for module 'M'

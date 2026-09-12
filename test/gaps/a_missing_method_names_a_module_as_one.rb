# The class a NameError names for a method lookup that finds nothing.
[[Comparable, :nope], [String, :nope], [5, :nope]].each do |recv, name|
  recv.method(name)
rescue NameError => e
  puts e.message
end
__END__
undefined method 'nope' for class 'Module'
undefined method 'nope' for class '#<Class:String>'
undefined method 'nope' for class 'Integer'

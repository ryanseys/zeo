# Class variables resolve through the LEXICAL scope, and a block opens no new
# one, so `@@cv1` there reaches the top level rather than the anonymous
# class: a RuntimeError naming toplevel access.
klass = Class.new do
  @@cv1 = 1
end
p klass.class_variables
__END__
#@ stderr
lang/classes/a_class_variable_assigned_inside_a_class_new_block.rb:5:in 'block in <main>': class variable access from toplevel (RuntimeError)
	from lang/classes/a_class_variable_assigned_inside_a_class_new_block.rb:4:in 'Class#initialize'
	from lang/classes/a_class_variable_assigned_inside_a_class_new_block.rb:4:in 'Class#new'
	from lang/classes/a_class_variable_assigned_inside_a_class_new_block.rb:4:in '<main>'
#@ exit 1

# A class variable (`@@cv`) assigned inside a `Class.new { ... }` block
# should raise RuntimeError ("class variable access from toplevel"): class
# variables resolve through the LEXICAL scope, and a `do...end` block (unlike
# a literal `class ... end`) doesn't open a new lexical scope, so `@@cv1`
# resolves to the top-level scope instead of the new anonymous class. zeo
# silently accepts the assignment instead of raising.
klass = Class.new do
  @@cv1 = 1
end
p klass.class_variables
__END__
#@ stderr
lang/classes/issue_class_variable_toplevel_scope.rb:8:in 'block in <main>': class variable access from toplevel (RuntimeError)
	from lang/classes/issue_class_variable_toplevel_scope.rb:7:in 'Class#initialize'
	from lang/classes/issue_class_variable_toplevel_scope.rb:7:in 'Class#new'
	from lang/classes/issue_class_variable_toplevel_scope.rb:7:in '<main>'
#@ exit 1

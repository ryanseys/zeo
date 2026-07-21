# Implicit-self send(:literal) / __send__ / public_send -> direct call (#1261).
# Done on the AST, so a `send(:` inside a string literal is NOT rewritten.
class Dispatcher
  def greet; "hi"; end
  def add(a, b); a + b; end
  def call_greet; send(:greet); end
  def call_add; send(:add, 2, 3); end
  def str_form; __send__("greet"); end
  def pub; public_send(:greet); end
end
d = Dispatcher.new
puts d.call_greet
puts d.call_add
puts d.str_form
puts d.pub
puts "x".send(:upcase)             # explicit receiver still works
puts "literal send(:foo) stays"    # string literal must be untouched

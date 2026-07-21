# A proc whose tail is an sp_RbVal- or sp_Class-valued raising expression
# (unresolved-call gate / NameError constant read) must coerce into the proc's
# mrb_int return carrier instead of returning the mismatched C type raw.
x = [1, "a"].sample
g = -> { x.no_such_method_here }   # gate: sp_raise_nomethod (sp_RbVal tail)
h = -> { NoSuchConst }             # NameError constant read (sp_Class dummy)
begin
  g.call
rescue Exception => e
  puts e.class
end
begin
  h.call
rescue Exception => e
  puts e.class
end
puts "done"

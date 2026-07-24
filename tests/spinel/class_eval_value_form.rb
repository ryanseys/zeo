# The value forms of class_eval / class_exec / module_eval / module_exec: the
# block is spliced inline (self = the receiver constant), its value is the
# call's value, and class_exec's args bind the block params (#2697). A pure-def
# body keeps going through the compile-time reopen path.
class C; end
module M; end
r = C.class_eval { 40 + 2 }
p r
p(C.class_exec(5) { |n| n * 2 })
r2 = C.class_eval do
  x = 10
  y = x * 2
  "#{self}/#{y}"
end
p r2
p(M.module_eval { 7 })
p(M.module_exec(3, 4) { |a, b| a + b })
base = 100
p(C.class_exec(5) { |n| base + n })
class D; end
D.class_eval do
  def hi; "hi"; end
end
p D.new.hi

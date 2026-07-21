class CompileTimeDirectDefineMethod
  define_method(:answer) { 42 }
  define_method("greeting") { "hello" }
end

obj = CompileTimeDirectDefineMethod.new
puts obj.answer
puts obj.greeting

class CompileTimeIntMethods
  [1, 2].each do |n|
    define_method("value_#{n}") { n }
  end
end

obj = CompileTimeIntMethods.new
puts obj.value_1
puts obj.value_2

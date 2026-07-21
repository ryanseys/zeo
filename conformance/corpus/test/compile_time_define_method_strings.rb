class CompileTimeStringMethods
  ["red", "blue"].each do |name|
    define_method("label_#{name}") { name }
  end
end

obj = CompileTimeStringMethods.new
puts obj.label_red
puts obj.label_blue

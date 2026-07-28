# Object#extend doesn't actually add the module's instance methods to the
# receiver's singleton class -- calling the extended method afterward raises
# NoMethodError.
m = Module.new do
  def helper
    "ext-help"
  end
end
obj = Object.new
obj.extend(m)
p obj.helper

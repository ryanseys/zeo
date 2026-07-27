def value(assign)
  param = "x" if assign
  param
end

raise "FAIL" unless value(false).nil?
p value(false)
p value(true)
x = "set" if false
p x
p x.nil?

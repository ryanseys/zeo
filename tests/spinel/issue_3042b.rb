p NoMethodError.new("m", :nm, [1, 2]).args
p NoMethodError.new("m", :nm, [1], true).private_call?
p NoMethodError.new("m", :nm, [1]).private_call?

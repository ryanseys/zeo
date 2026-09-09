# NoMethodError built with arguments answers #args, and #private_call? answers
# the flag it was given or false.
# (spinel issue #3042b)
p NoMethodError.new("m", :nm, [1, 2]).args
p NoMethodError.new("m", :nm, [1], true).private_call?
p NoMethodError.new("m", :nm, [1]).private_call?
__END__
[1, 2]
true
false

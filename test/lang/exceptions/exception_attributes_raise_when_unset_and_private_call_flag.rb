# KeyError#key / #receiver and NameError#receiver raise ArgumentError when
# never set, but a real miss (nil.foo, h.fetch) sets them. NoMethodError's
# 4th positional is the private_call? flag.

p((begin; KeyError.new("m").key; rescue => e; e.class; end))
p((begin; KeyError.new("m").receiver; rescue => e; e.class; end))
p((begin; NoMethodError.new("m").receiver; rescue => e; e.class; end))
p((begin; NameError.new("m", :n).receiver; rescue => e; e.class; end))
p((begin; KeyError.new("m", key: :k, receiver: {}).key; rescue => e; e.class; end))
p((begin; nil.foo; rescue => e; e.receiver; end))
h = { a: 1 }
p((begin; h.fetch(:z); rescue => e; e.key; end))
p NoMethodError.new("m", :nm, [1], true).private_call?
p NoMethodError.new("m", :nm, [1]).private_call?
p NoMethodError.new("m", :nm, [1, 2]).args
__END__
ArgumentError
ArgumentError
ArgumentError
ArgumentError
:k
nil
:z
true
false
[1, 2]

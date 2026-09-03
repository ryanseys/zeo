# `Struct.new` in ANY position mints a native struct class at runtime: an
# anonymous local/inline struct is a runtime value, exactly like the
# constant form -- there is no compile-time-synthesized path.

k = Struct.new(:a, :b)
o = k.new(1, 2)
p o
p o.a
p o.to_a
p o.members
o.a = 9
p o.a
__END__
#<struct a=1, b=2>
1
[1, 2]
[:a, :b]
9

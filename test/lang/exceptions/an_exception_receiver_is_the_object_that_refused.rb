# NoMethodError and KeyError each carry the receiver that raised; one built by hand does not.
# (spinel issue #3036)
r001 = (NameError.new("m", :n).receiver rescue $!.class)
p r001
r002 = (nil.foo rescue $!.receiver)
p r002
h = {a: 1}
r003 = (h.fetch(:z) rescue $!.receiver)
p r003
__END__
ArgumentError
nil
{a: 1}

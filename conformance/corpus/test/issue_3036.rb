r001 = (NameError.new("m", :n).receiver rescue $!.class)
p r001
r002 = (nil.foo rescue $!.receiver)
p r002
h = {a: 1}
r003 = (h.fetch(:z) rescue $!.receiver)
p r003

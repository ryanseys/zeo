# Reflection wide enough to produce ANY class is a hatch, not an edge: no
# scan of the source bounds what these can name, so the compiler stops
# narrowing the builtin method tables and carries all of them
# (`analyze::class_reach::hands_out_every_class`).
#
# Each block below names its class only THROUGH the reflective surface, so
# the narrowed set would have dropped it.

p Marshal.load(Marshal.dump({ a: [1, 2] }))

name = ["Path", "name"].join
p Object.const_get(name).public_send(:new, "/tmp").basename.to_s

verb = ["to", "_c"].join
p 2.send(verb)

p Object.constants.include?(:Comparable)

seen = []
ObjectSpace.each_object(Class) { |c| seen << c }
p seen.is_a?(Array)
__END__
{a: [1, 2]}
"tmp"
(2+0i)
true
true

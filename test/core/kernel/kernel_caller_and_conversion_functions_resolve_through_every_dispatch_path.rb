# `caller`/`caller_locations` answer an empty Array (no runtime frames in an
# AOT build), and the private Kernel conversion/format helpers resolve as
# real methods -- so a splat call, `method(:Integer)`, or a forwarded block
# reach them, not only the codegen fast-path. Byte-verified against ruby 4.0.6.

def frames; caller; end
p frames.is_a?(Array)
p caller_locations(1, 1).is_a?(Array)
args = ["%d-%s", 3, "x"]
puts format(*args)
p [1, 2, 3].map(&method(:Integer))
p ["1", "0xff"].map(&method(:Integer))
def wrap(&b); proc(&b); end
p wrap { |x| x + 100 }.call(1)
__END__
true
true
3-x
[1, 2, 3]
[1, 255]
101

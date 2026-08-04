# `Marshal` must round-trip a value to something `==` to it. Two shapes come
# back wrong:
#
#   * a `Range` cannot be dumped at all -- `TypeError: no _dump_data is defined
#     for class Range` -- so any structure CONTAINING one fails with it;
#   * a `Hash`'s default value is dropped, so `Hash.new(0)` loads back with a
#     nil default and the first `h[k] += 1` against it raises.
#
# Ruby marshals a Range as an ordinary object with three ivars (`@begin`,
# `@end`, `@excl`) and writes a hash's default alongside its entries
# (`marshal.c`'s `w_object` has a dedicated `TYPE_HASH_DEF`). zeo's dumper
# reaches neither, and the Range case surfaces as an error about a C-extension
# hook that has nothing to do with what the caller wrote.
#
# Marshal is how a value crosses a process or a cache boundary, so a silently
# dropped default is the worse of the two: it fails later, somewhere else.

p Marshal.load(Marshal.dump(1..2))
p Marshal.load(Marshal.dump([1, (1...5), :x]))

p Marshal.load(Marshal.dump({a: 1, b: [2, 3]}))

h = Hash.new(0)
h[:seen] = 1
loaded = Marshal.load(Marshal.dump(h))
p loaded
p loaded[:missing]

# These already round-trip, so a fix must keep them.
p Marshal.load(Marshal.dump([1, "a", :b, nil, true, 1.5]))
s = +"shared"
pair = Marshal.load(Marshal.dump([s, s]))
p pair[0].equal?(pair[1])
cyc = [1]
cyc << cyc
p Marshal.load(Marshal.dump(cyc))[1].class

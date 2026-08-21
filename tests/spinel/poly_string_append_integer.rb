# `str << int` appends that CODEPOINT, and one outside the range is a
# RangeError. A String reached through a POLY slot -- the same local also
# assigned from a container read, so its type is the union -- fell past the
# String arms of the boxed `<<` and was read as a NUMBER, so the append
# answered an integer shift: `a << -1` gave 0 where CRuby raises (#4015).
a = +"a"
r = (a << -1 rescue $!.class)
p r
h = { k: +"abc" }
a = h[:k]
a << "Z"
p h

# the same local, both directions
b = { k: +"abc" }
c = b[:k]
p (c << -1 rescue $!.class)
p (c << 66 rescue $!.class)
p (c << 0x3042 rescue $!.class)
p (c << 2**40 rescue $!.class)
p b

# through an Array element
f = [+"q"]
g = f[0]
p (g << -5 rescue $!.class)
p (g << 33 rescue $!.class)
p f

# a shared handle appends to a shared handle
i = [+"x"]
j = [+"y"]
k = i[0]
k << j[0]
p i

# and the typed route keeps answering what it answered
m = +"a"
p (m << 66 rescue $!.class)
p (m << -1 rescue $!.class)

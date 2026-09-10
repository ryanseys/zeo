require "tmpdir"
ZTMP = Dir.mktmpdir

s = "a\0b"
puts s.length
puts s.bytes.inspect
puts (s == "a\0b")
puts (s == "a\0c")
puts (s == "a")
h = {}
h[s] = 1
h["a\0c"] = 2
h["a"] = 3
puts h.size
d = s.dup
puts d.bytes.inspect
puts (d == s)
t = "x" + 0.chr + "y"
e = t[0, 3]
puts e.bytes.inspect
puts e.length
puts s[2]
f = File.join(ZTMP, "probe_nul_core_t.bin")
File.write(f, s)
r = File.read(f)
puts r.bytes.inspect
puts (r == s)
File.delete(f)
__END__
3
[97, 0, 98]
true
false
false
3
[97, 0, 98]
true
[120, 0, 121]
3
b
[97, 0, 98]
true

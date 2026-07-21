# Int-keyed, int-valued hash literals build a native IntIntHash
# (previously misrouted to StrIntHash, passing int keys as char* and
# crashing). Reads, []= writes, and length all work.
h = {1 => 10, 2 => 20, 3 => 30}
puts h[1]
puts h[2]
puts h[3]
h[4] = 40
puts h[4]
puts h.length
h[2] = 222
puts h[2]
puts h.length

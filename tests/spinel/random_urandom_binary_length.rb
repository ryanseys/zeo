# Random.urandom hands back binary bytes, so #length is the byte count. spinel
# keeps no per-string encoding tag and counts UTF-8 code points whenever the
# bytes happen to spell valid UTF-8 -- true of about 1.5% of 4-byte draws, which
# then reported a length short of the requested size. #3474.
p 2000.times.any? { Random.urandom(4).length != 4 }
p 2000.times.any? { Random.urandom(4).bytesize != 4 }
p 2000.times.any? { Random.urandom(4).bytes.size != 4 }
p 2000.times.any? { Random.urandom(4).chars.size != 4 }
p 2000.times.any? { Random.urandom(4).size != 4 }
p Random.urandom(0).length
p Random.urandom(32).length
s = Random.urandom(8)
p s.length == s.bytesize
p s.dup.length == 8
p s[0, 8].bytesize

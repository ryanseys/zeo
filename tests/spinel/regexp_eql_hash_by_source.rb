a001 = Regexp.new("ab")
p(a001 == /ab/)
p(a001.eql?(/ab/))
p(a001.hash == /ab/.hash)
h001 = { /ab/ => 1 }
p h001[a001]

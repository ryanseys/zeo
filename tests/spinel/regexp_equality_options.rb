# Regexp equality is source AND options: /ab/ and /ab/i are different patterns.
# Only the source was compared, so every flag variant of a pattern was equal to
# it. #eql? is value equality too (only #equal? is identity), and it was
# answering from a pointer comparison.
p(/ab/ == /ab/i)
p(/ab/ == /ab/m)
a = /ab/
b = /ab/x
p(a == b)
p(/ab/.eql?(/ab/i))
p(/ab/ == /ab/)
p(/ab/.eql?(/ab/))
p(/ab/i == /ab/i)
p(/ab/i.eql?(/ab/i))
p(/ab/ != /ab/i)
p(/ab/ != /ab/)
p(/ab/ == /cd/)
p(/ab/im == /ab/mi)

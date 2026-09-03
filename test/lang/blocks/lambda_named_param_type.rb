# The cycle this program builds on purpose, and the census the
# `ZEO_RT_GCCHECK=1` leg gates against. A cycle alive at exit is not a
# defect: it is a ring the program never broke. What the leg gates is a
# CHANGE to the line below.
# 
# A Holder holds a lambda that closed over the Holder.
#@ gccheck: cycle leak: 2 objects (Holder x1, Proc x1)
IDX  = ->(v) { v[0] }
SIZE = ->(v) { v.size }
SUM  = ->(v) { v.sum }
p IDX.call([7])
p SIZE.call([7, 8])
p SUM.call([1, 2, 3])

class Holder
  def initialize
    @fn = ->(v) { v.size }
  end
  def run(x)
    @fn.call(x)
  end
end
p Holder.new.run([1, 2, 3])

STR = ->(s) { s.upcase }
p STR.call("ab")
p STR.("cd")
p STR["ef"]

CONST_LAMBDA = ->(v) { v.class.to_s }
local_lambda = ->(v) { v.class.to_s }
p CONST_LAMBDA.call([7])
p local_lambda.call([7])

RE_F = ->(re) { re.source }
p RE_F.call(/b/)
__END__
7
2
6
3
"AB"
"CD"
"EF"
"Array"
"Array"
"b"

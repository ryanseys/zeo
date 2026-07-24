# Object#method / #public_method with an undefined name raise NameError
# immediately; a defined name still returns a callable Method.
r = (true.method(:nope) rescue $!.class); p r
r2 = ("s".method(:zzz) rescue $!.class); p r2
r3 = ([1].method(:qqq) rescue $!.class); p r3
r4 = (nil.method(:blah) rescue $!.class); p r4
m = "abc".method(:upcase)
p m.class
p m.call
class UU; def go; 7; end; end
r5 = (UU.new.method(:nope2) rescue $!.class); p r5
p UU.new.method(:go).call
r6 = (5.public_method(:nope3) rescue $!.class); p r6

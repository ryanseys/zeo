# A reader whose name is also a String method, read through a poly receiver.
# The poly shortcuts stringified the receiver and applied the String method, so
# a Struct member called `upcase` answered the UPCASED #inspect of the object
# holding it. Declining to the general dispatch is not enough on its own: that
# dispatch only covers SP_TAG_OBJ, so a genuine String receiver in the same
# program then fell through to the slot's seed -- nil, or 0 for #bytes.
E = Struct.new(:ip, :upcase, :downcase, :capitalize, :swapcase,
               :strip, :reverse, :chomp, :chop, :succ, :chr, :bytes, :chars)
e = E.new("a", 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12)

# the member wins, inside a block and through &:sym
[e].each { |x| p [x.upcase, x.downcase, x.capitalize, x.swapcase] }
[e].each { |x| p [x.strip, x.reverse, x.chomp, x.chop] }
[e].each { |x| p [x.succ, x.chr, x.bytes, x.chars] }
p [e].map(&:upcase)
p [e].map(&:bytes)

D = Data.define(:upcase, :bytes)
p [D.new(upcase: 7, bytes: 8)].map { |d| [d.upcase, d.bytes] }

class C
  attr_accessor :upcase, :reverse, :chars
  def initialize; @upcase = 1; @reverse = 2; @chars = 3; end
end
p [C.new].map { |o| [o.upcase, o.reverse, o.chars] }

# ... and a real String receiver still answers String, in the same program.
# A mixed array makes the element genuinely poly, which is the case that broke.
mixed = ["  Ab  ", 65]
p mixed[0].upcase
p mixed[0].downcase
p mixed[0].capitalize
p mixed[0].swapcase
p mixed[0].strip
p mixed[0].reverse
p mixed[0].bytes
p mixed[0].chars
p mixed[1].chr
p ["x\n", 1][0].chomp
p ["xy", 1][0].chop
p ["az", 1][0].succ

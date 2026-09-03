# `Data.define` builds a class whose two constructor forms are the SAME
# constructor: ruby converts positional arguments to keywords and reports a
# shortfall as a missing KEYWORD, never as an arity error.
#
#     Coord.new(1)  ->  ArgumentError: missing keyword: :lng
#
# zeo reports "wrong number of arguments (given 1, expected 2)", which is
# `Struct`'s message -- and `Struct` really does allow the short form
# (`S.new(1)` leaves `b` nil), so the two classes need different bodies here.
# `Data` is the immutable half of that pair, and refusing an incomplete
# instance is the whole reason it exists.
#
# The memberless case has a stray space: zeo inspects `Data.define.new` as
# `#<data D2 >`, ruby as `#<data D2>` -- the separator is emitted before the
# member list rather than between members.

Coord = Data.define(:lat, :lng)

begin
  Coord.new(1)
rescue ArgumentError => e
  puts "#{e.class}: #{e.message}"
end

begin
  Coord.new(1, 2, 3)
rescue ArgumentError => e
  puts "#{e.class}: #{e.message}"
end

begin
  Coord.new(lat: 1)
rescue ArgumentError => e
  puts "#{e.class}: #{e.message}"
end

Empty = Data.define
p Empty.new
p Empty.new.inspect

# Struct keeps the short form and the arity message -- these must not change.
Pair = Struct.new(:a, :b)
p Pair.new(1)
__END__
ArgumentError: missing keyword: :lng
ArgumentError: wrong number of arguments (given 3, expected 0..2)
ArgumentError: missing keyword: :lng
#<data Empty>
"#<data Empty>"
#<struct Pair a=1, b=nil>

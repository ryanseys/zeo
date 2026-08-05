# A converter's `#primitive_convert`, given a `dst_bytesize` limit, stops at
# a different point than CRuby does. (This first line avoids naming the class
# outright: ruby would read `coding:` inside it as a magic comment.)
#
# zeo refuses the first character whose bytes would not fit and consumes
# nothing more, so the source keeps every character that did not convert.
# CRuby converts further than the limit and holds the overflow in an internal
# output buffer, so its source is shorter and the extra bytes arrive on the
# NEXT call. Where the cut falls is a property of CRuby's buffer sizes, not
# of the conversion.
#
# The end state after draining is the same either way -- the same bytes reach
# the destination in the same order -- so this is an intermediate observable
# only. Fix shape: give `ConvState` a third buffer holding output produced
# past the limit, and drain it ahead of the next conversion.

def show(label)
  puts "#{label}: #{yield.inspect}"
rescue Exception => e # rubocop:disable Lint/RescueException
  puts "#{label}: #{e.class}: #{e.message}"
end

show("cut point") do
  c = Encoding::Converter.new("UTF-8", "EUC-JP")
  src = +"あいう"
  dst = +""
  [c.primitive_convert(src, dst, 0, 2), src.bytes, dst.bytes]
end

# Draining reaches the same place: all six bytes, in order.
show("drained") do
  c = Encoding::Converter.new("UTF-8", "EUC-JP")
  src = +"あいう"
  dst = +""
  c.primitive_convert(src, dst, 0, 2)
  [c.primitive_convert(src, dst, dst.bytesize, 10), src.bytes, dst.bytes]
end

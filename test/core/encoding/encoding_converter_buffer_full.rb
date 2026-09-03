# A converter's `#primitive_convert` cuts at CRuby's own point under a
# `dst_bytesize` limit. (This first line avoids naming the class outright:
# ruby would read `coding:` inside it as a magic comment.)
#
# This was a gap. zeo used to refuse the first character whose bytes would not
# fit and consume nothing more, so its source kept every character that did
# not convert while CRuby's was shorter. `ConvState` now holds the output
# produced past the limit and drains it ahead of the next conversion, which is
# what makes the INTERMEDIATE state agree and not just the end state.
#
# Both rows below are pinned: where the cut falls, and what a drain answers.

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
__END__
cut point: [:destination_buffer_full, [227, 129, 134], [164, 162]]
drained: [:finished, [], [164, 162, 164, 164, 164, 166]]

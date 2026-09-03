# `unpack`/`unpack1` accept `offset:`; `offset == bytesize` yields the
# empty tail (nil), past-the-end raises, negative raises.

pair = [1.5, 2.25].pack("G2")
p pair.unpack1("G", offset: 8)
p pair.unpack("G", offset: 8)
p "abc".unpack("C", offset: 3)
begin; "abc".unpack("C", offset: 5); rescue ArgumentError => e; puts e.message; end
begin; "abc".unpack("C", offset: -1); rescue ArgumentError => e; puts e.message; end
__END__
2.25
[2.25]
[nil]
offset outside of string
offset can't be negative

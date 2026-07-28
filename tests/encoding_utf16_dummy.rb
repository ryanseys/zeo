# The dummy UTF-16/UTF-32 encodings (no endianness in the name): encoding
# TO one writes a BOM then big-endian code units; decoding FROM one reads
# the endianness off the BOM, and without a BOM the bytes are an invalid
# sequence. The tag is the dummy encoding itself, not the BE/LE row.
p "hello".encode("UTF-16")
s = "あ".encode("UTF-16")
p [s.encoding, s.unpack1("H*"), s.encoding.dummy?]
p s.encode("UTF-8")
p "あ".encode("UTF-32").unpack1("H*")
p "\xFE\xFF\x30\x42".b.force_encoding("UTF-16").encode("UTF-8")
begin
  "a\x00".b.force_encoding("UTF-16").encode("UTF-8")
rescue => e
  p [e.class, e.message]
end
p Encoding::UTF_16.names
p [Encoding::UTF_16.dummy?, Encoding::UTF_16BE.dummy?]

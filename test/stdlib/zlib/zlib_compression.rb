require "zlib"

# deflate / inflate round-trip (and inflate is byte-compatible with any zlib).
text = "The quick brown fox jumps over the lazy dog. " * 3
compressed = Zlib.deflate(text)
p compressed.encoding.name
p Zlib.inflate(compressed) == text
p Zlib.inflate(compressed).length

# Compression levels 0 (store) .. 9 (best) all round-trip.
p [0, 1, 6, 9].map { |lvl| Zlib.inflate(Zlib.deflate(text, lvl)) == text }

# gzip / gunzip round-trip.
gz = Zlib.gzip(text)
p Zlib.gunzip(gz) == text

# inflate can read a stream produced by another zlib implementation.
external = [120, 156, 203, 72, 205, 201, 201, 87, 40, 207, 47, 202, 73, 1, 0, 26, 11, 4, 93].pack("C*")
p Zlib.inflate(external)
__END__
"ASCII-8BIT"
true
135
[true, true, true, true]
true
"hello world"

# `str.encode(enc)` where str is ALREADY in enc is a no-op in ruby -- no byte
# validation happens, invalid bytes ride through untouched. zeo runs the
# transcoder anyway and raises InvalidByteSequenceError on the bad bytes.
s = "\xFF\xFE".dup.force_encoding("UTF-8")
r = s.encode("UTF-8", "UTF-8")
puts r.bytes.inspect
puts r.encoding

t = "ok".encode("UTF-8")
puts t

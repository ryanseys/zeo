# ruby's concatenation negotiation for an EMPTY receiver: an
# ascii-compatible empty string keeps its OWN encoding when the addition is
# 7-bit ("".b << "abc" stays BINARY), and adopts the addition's encoding
# only when the addition needs it -- including a wide encoding, whose bytes
# may all be 7-bit and whose tag still wins.
def row(l, r)
  s = l.dup
  s << r
  [s.encoding.to_s, s.bytesize]
rescue Encoding::CompatibilityError
  [:compat_error]
end

p row("".b, "hello")
p row("".b, "héllo")
p row("x".b, "hello")
p row("x".b, "héllo")
p row(+"", "hi".b)
p row(+"", "h\xC3\xA9".b)
p row(+"abc", "h\xC3\xA9".b)
p row(+"abc", "hi".b)
p row("é".b, "hé")
p row(+"héllo", "".b)
p row("".encode("US-ASCII"), "hé")
p row("".encode("US-ASCII"), "hi")
p row("".b, "hi".encode("US-ASCII"))
p row("".encode("UTF-16LE"), "abc")
p row("".b, "ab".encode("UTF-16LE"))
p row("".b, "".encode("UTF-16LE"))
p row("".encode("EUC-JP"), "héllo")
p row("".b, "héllo".encode("EUC-JP"))
p [("x".b << 233).encoding.to_s, ("x".b << 233).bytes]
p [(+"x" << 233).encoding.to_s, (+"x" << 233).bytes]
__END__
["ASCII-8BIT", 5]
["UTF-8", 6]
["ASCII-8BIT", 6]
["UTF-8", 7]
["UTF-8", 2]
["ASCII-8BIT", 3]
["ASCII-8BIT", 6]
["UTF-8", 5]
[:compat_error]
["UTF-8", 6]
["UTF-8", 3]
["US-ASCII", 2]
["ASCII-8BIT", 2]
["UTF-8", 3]
["UTF-16LE", 4]
["ASCII-8BIT", 0]
["UTF-8", 6]
["EUC-JP", 7]
["ASCII-8BIT", [120, 233]]
["UTF-8", [120, 195, 169]]

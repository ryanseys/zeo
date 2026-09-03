# `String#encode`'s decorators, and where they land in a NON-ascii target.
# The replacement text and the XML markup are both rendered INTO the target,
# so a UTF-16 result gets whole code units rather than stray ASCII bytes --
# and `xml: :attr` writes a whole attribute, quotes included.

def show(label)
  puts "#{label}: #{yield.inspect}"
rescue Exception => e # rubocop:disable Lint/RescueException
  puts "#{label}: #{e.class}: #{e.message}"
end

show("xml attr") { %(a"b<c).encode("EUC-JP", xml: :attr) }
show("xml text") { %(a"b<c).encode("EUC-JP", xml: :text) }
show("xml attr undef") { "あ".encode("US-ASCII", xml: :attr) }
show("universal") { "a\r\nb\rc".encode("EUC-JP", universal_newline: true) }
show("crlf") { "a\nb".encode("EUC-JP", crlf_newline: true) }
show("cr") { "a\nb".encode("EUC-JP", cr_newline: true) }
show("utf16 invalid replace") do
  "a\xffb".dup.force_encoding("UTF-8").encode("UTF-16BE", invalid: :replace).bytes
end
show("utf32 invalid replace") do
  "a\xffb".dup.force_encoding("UTF-8").encode("UTF-32BE", invalid: :replace).bytes
end
show("utf16 xml attr") { %(a"b).encode("UTF-16BE", xml: :attr).bytes }
show("utf16 undef replace") { "あ".encode("US-ASCII", undef: :replace) }
show("replace text into utf16") do
  "a\xffb".dup.force_encoding("UTF-8").encode("UTF-16BE", invalid: :replace, replace: "!").bytes
end
__END__
xml attr: "\"a&quot;b&lt;c\""
xml text: "a\"b&lt;c"
xml attr undef: "\"&#x3042;\""
universal: "a\nb\nc"
crlf: "a\r\nb"
cr: "a\rb"
utf16 invalid replace: [0, 97, 255, 253, 0, 98]
utf32 invalid replace: [0, 0, 0, 97, 0, 0, 255, 253, 0, 0, 0, 98]
utf16 xml attr: [0, 34, 0, 97, 0, 38, 0, 113, 0, 117, 0, 111, 0, 116, 0, 59, 0, 98, 0, 34]
utf16 undef replace: "?"
replace text into utf16: [0, 97, 0, 33, 0, 98]

# Every CGI escape is a BYTE transformation: it rewrites ASCII punctuation
# and passes every other byte through, so ruby hands back a string in the
# encoding it was given -- invalid bytes and all.
#
# Decoding the argument first breaks that twice over. It re-encodes (a byte
# read as a codepoint and written back as UTF-8 turns a snowman into three
# Latin-1 characters), and it loses the encoding (a binary string comes
# back UTF-8). Both were live: `CGI.escapeHTML("\xC0<")` answered
# `"À&lt;"` in UTF-8, and rack's `escape_html` specs are what caught it.
require "cgi/escape"

snow = "☃"
bin  = "\xC0<".dup.force_encoding("BINARY")
lat  = "caf\xE9".dup.force_encoding("ISO-8859-1")

[snow, bin, lat].each do |s|
  r = CGI.escapeHTML(s)
  puts "escapeHTML(#{s.encoding}) -> #{r.inspect} #{r.encoding}"
end

[[snow, "escape"], [bin, "escape"], [snow, "escapeURIComponent"]].each do |s, m|
  r = CGI.public_send(m, s)
  puts "#{m}(#{s.encoding}) -> #{r.inspect} #{r.encoding}"
end

# A percent-decode answers BYTES: `%C0` is not valid UTF-8 and survives
# anyway, where a lossy decode replaced it with U+FFFD.
r = CGI.unescape("%C0%3C")
puts "unescape -> #{r.inspect} #{r.encoding}"

# A named entity is ASCII and always decodes. A NUMERIC one decodes only
# when the string's encoding can hold the character, and is left standing
# when it cannot: no byte in a binary string is a snowman, and Latin-1 has
# a byte for `&#233;` but none for `&#9731;`.
{
  "utf8"   => "UTF-8",
  "binary" => "BINARY",
  "latin1" => "ISO-8859-1",
}.each do |label, enc|
  ["&lt;&amp;", "&#9731;", "&#233;"].each do |src|
    r = CGI.unescapeHTML(src.dup.force_encoding(enc))
    puts "unescapeHTML(#{label}, #{src}) -> #{r.inspect} #{r.encoding}"
  end
end

# The element forms escape only the tags they were asked about, and leave
# the bytes around them alone. The span rule itself -- where one tag ends --
# is `tests/cgi_element_escapes_span_one_tag.rb`; what this file asks of the
# family is that it stay a BYTE transformation.
doc = "<A HREF='x'>t</A>\xC0".dup.force_encoding("BINARY")
r = CGI.escapeElement(doc, "A")
puts "escapeElement -> #{r.inspect} #{r.encoding}"
puts CGI.unescapeElement("&lt;A&gt;t&lt;/A&gt;\xC0".dup.force_encoding("BINARY"), "A").inspect

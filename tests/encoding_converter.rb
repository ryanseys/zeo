# The stateful face of the same engine `String#encode` runs on -- one
# `Encoding::Converter`, fed a piece at a time. Note the wording of this
# first line: `# Encoding::Converter` would read as a magic comment, since
# ruby sees `coding:` in it and tries the encoding named `:Converter`.
#
# The difference from `String#encode` is only WHERE a conversion can stop, so this
# exercises every way it can -- a chunk boundary inside a character, a
# malformed sequence, a character the target cannot hold, and a full
# destination buffer.

EC = Encoding::Converter

def show(label)
  puts "#{label}: #{yield.inspect}"
rescue Exception => e # rubocop:disable Lint/RescueException
  puts "#{label}: #{e.class}: #{e.message}"
end

show("own singleton") { EC.singleton_methods(false).sort }
show("own instance") { EC.instance_methods(false).sort }
show("constants") { EC.constants.sort }
show("INVALID_MASK") { EC::INVALID_MASK }
show("UNDEF_HEX_CHARREF") { EC::UNDEF_HEX_CHARREF }
show("PARTIAL_INPUT") { EC::PARTIAL_INPUT }
show("AFTER_OUTPUT") { EC::AFTER_OUTPUT }
show("XML_ATTR_QUOTE_DECORATOR") { EC::XML_ATTR_QUOTE_DECORATOR }

# Construction, and every way it refuses.
show("new") { EC.new("UTF-8", "EUC-JP") }
show("new encodings") { EC.new(Encoding::UTF_8, Encoding::EUC_JP) }
show("new one arg") { EC.new("UTF-8") }
show("new symbol") { EC.new(:"UTF-8", :"EUC-JP") }
show("new unknown") { EC.new("nope", "UTF-8") }
show("new same") { EC.new("UTF-8", "UTF-8") }
show("new same + decorator") { EC.new("UTF-8", "UTF-8", universal_newline: true) }
show("new two newlines") { EC.new("UTF-8", "EUC-JP", crlf_newline: true, universal_newline: true) }
show("new no-conversion") { EC.new("", "") }
show("new decorator only") { EC.new("", "", universal_newline: true) }

# What it says about itself.
show("source_encoding") { EC.new("UTF-8", "EUC-JP").source_encoding }
show("destination_encoding") { EC.new("UTF-8", "EUC-JP").destination_encoding }
show("no-conversion source") { EC.new("", "").source_encoding }
show("inspect") { EC.new("UTF-8", "EUC-JP").inspect }
show("inspect decorated") { EC.new("UTF-8", "EUC-JP", crlf_newline: true).inspect }
show("inspect xml attr") { EC.new("UTF-8", "EUC-JP", xml: :attr).inspect }
show("inspect no-conversion") { EC.new("", "").inspect }
show("inspect decorator only") { EC.new("", "", crlf_newline: true).inspect }
show("== same") { EC.new("UTF-8", "EUC-JP") == EC.new("UTF-8", "EUC-JP") }
show("== different") { EC.new("UTF-8", "EUC-JP") == EC.new("UTF-8", "UTF-16BE") }
show("== non-converter") { EC.new("UTF-8", "EUC-JP") == 1 }

# The converter path: one hop where a direct converter exists, a pivot
# through UTF-8 where it does not, and three hops for the stateful JIS row.
show("convpath direct") { EC.new("UTF-8", "EUC-JP").convpath }
show("convpath pivot") { EC.search_convpath("ISO-8859-1", "EUC-JP") }
show("convpath sjis-euc") { EC.search_convpath("Shift_JIS", "EUC-JP") }
show("convpath jis") { EC.search_convpath("ISO-2022-JP", "UTF-8") }
show("convpath long") { EC.search_convpath("Windows-1252", "ISO-2022-JP") }
show("convpath decorated") { EC.search_convpath("UTF-8", "EUC-JP", universal_newline: true) }
show("convpath xml") { EC.search_convpath("UTF-8", "EUC-JP", xml: :attr) }
show("convpath same") { EC.search_convpath("UTF-8", "UTF-8") }
show("convpath decorator only") { EC.new("", "", crlf_newline: true).convpath }

show("asciicompat 2022") { EC.asciicompat_encoding("ISO-2022-JP") }
show("asciicompat utf16") { EC.asciicompat_encoding(Encoding::UTF_16BE) }
show("asciicompat ibm037") { EC.asciicompat_encoding("IBM037") }
show("asciicompat cesu8") { EC.asciicompat_encoding("CESU-8") }
show("asciicompat utf8") { EC.asciicompat_encoding("UTF-8") }
show("asciicompat utf8-mac") { EC.asciicompat_encoding("UTF8-MAC") }
show("asciicompat unknown") { EC.asciicompat_encoding("nope") }

# Converting, one chunk at a time.
show("convert") { EC.new("UTF-8", "EUC-JP").convert("あい").bytes }
show("convert encoding") { EC.new("UTF-8", "EUC-JP").convert("a").encoding }
show("convert split") do
  c = EC.new("UTF-8", "EUC-JP")
  [c.convert("\xE3\x81").bytes, c.convert("\x82").bytes, c.finish.bytes]
end
show("convert undef") { EC.new("UTF-8", "US-ASCII").convert("é") }
show("convert undef replaced") { EC.new("UTF-8", "US-ASCII", undef: :replace).convert("é") }
show("convert invalid") { EC.new("UTF-8", "EUC-JP").convert("\xFF") }
show("convert invalid replaced") { EC.new("UTF-8", "EUC-JP", invalid: :replace).convert("\xFF").bytes }
show("convert after finish") do
  c = EC.new("UTF-8", "EUC-JP")
  c.finish
  c.convert("a")
end
show("convert non-string") { EC.new("UTF-8", "EUC-JP").convert(1) }
show("finish truncated") do
  c = EC.new("UTF-8", "EUC-JP")
  c.convert("a\xE3\x81")
  c.finish
end
show("finish truncated replaced") do
  c = EC.new("UTF-8", "EUC-JP", invalid: :replace)
  c.convert("a\xE3\x81")
  c.finish.bytes
end
show("finish closes jis") do
  c = EC.new("UTF-8", "ISO-2022-JP")
  [c.convert("あ").bytes, c.finish.bytes]
end

# The decorators.
show("crlf") { EC.new("UTF-8", "EUC-JP", crlf_newline: true).convert("a\nb").bytes }
show("cr") { EC.new("UTF-8", "EUC-JP", cr_newline: true).convert("a\nb").bytes }
show("universal") { EC.new("", "", universal_newline: true).convert("a\r\nb") }
show("universal cr only") { EC.new("", "", universal_newline: true).convert("a\rb") }
show("universal split") do
  c = EC.new("", "", universal_newline: true)
  [c.convert("a\r"), c.convert("\nb")]
end
show("xml text") { EC.new("UTF-8", "EUC-JP", xml: :text).convert("<a&b>").bytes }
show("xml attr") { EC.new("UTF-8", "EUC-JP", xml: :attr).convert(%(a"b)).bytes }
show("flags integer") do
  EC.new("UTF-8", "EUC-JP", EC::INVALID_REPLACE | EC::UNDEF_REPLACE).convert("\xFF").bytes
end
show("flags universal") { EC.new("", "", EC::UNIVERSAL_NEWLINE_DECORATOR).convert("a\r\nb") }

# The replacement text.
show("replacement euc") { EC.new("UTF-8", "EUC-JP").replacement }
show("replacement euc encoding") { EC.new("UTF-8", "EUC-JP").replacement.encoding }
show("replacement utf8") { EC.new("EUC-JP", "UTF-8").replacement }
show("replacement utf16") { EC.new("UTF-8", "UTF-16BE").replacement.bytes }
show("replacement set") do
  c = EC.new("UTF-8", "US-ASCII", undef: :replace)
  c.replacement = "!"
  [c.replacement, c.convert("é")]
end
show("replacement unrepresentable") do
  c = EC.new("UTF-8", "US-ASCII")
  c.replacement = "あ"
end

# The low-level form names how it stopped instead of raising.
show("primitive whole") do
  c = EC.new("UTF-8", "EUC-JP")
  src = +"あいう"
  dst = +""
  [c.primitive_convert(src, dst), src.bytes, dst.bytes]
end
show("primitive truncated") do
  c = EC.new("UTF-8", "EUC-JP")
  src = +"a\xE3\x81"
  dst = +""
  [c.primitive_convert(src, dst), src.bytes, dst.bytes, c.primitive_errinfo]
end
show("primitive partial input") do
  c = EC.new("UTF-8", "EUC-JP")
  src = +"a\xE3\x81"
  dst = +""
  r = c.primitive_convert(src, dst, nil, nil, EC::PARTIAL_INPUT)
  [r, src.bytes, dst.bytes, c.primitive_errinfo]
end
show("primitive invalid") do
  c = EC.new("UTF-8", "EUC-JP")
  src = +"a\xFFb"
  dst = +""
  [c.primitive_convert(src, dst), src.bytes, dst.bytes, c.primitive_errinfo, c.last_error.class]
end
show("primitive undefined") do
  c = EC.new("UTF-8", "US-ASCII")
  src = +"aéb"
  dst = +""
  [c.primitive_convert(src, dst), src.bytes, dst.bytes, c.primitive_errinfo]
end
show("primitive offset") do
  c = EC.new("UTF-8", "EUC-JP")
  src = +"あ"
  dst = +"keep"
  [c.primitive_convert(src, dst, 2), dst.bytes]
end
show("errinfo fresh") { EC.new("UTF-8", "EUC-JP").primitive_errinfo }
show("last_error fresh") { EC.new("UTF-8", "EUC-JP").last_error }
show("putback") { EC.new("UTF-8", "EUC-JP").putback.bytes }

show("insert_output") do
  c = EC.new("UTF-8", "EUC-JP")
  [c.insert_output("X"), c.convert("a").bytes]
end
show("insert_output converts") do
  c = EC.new("UTF-8", "EUC-JP")
  c.insert_output("あ")
  c.finish.bytes
end

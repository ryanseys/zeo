# A Ruby String is bytes PLUS an encoding that says how to read them. The
# Encoding engine here supports UTF-8, US-ASCII, ASCII-8BIT (BINARY), and
# ISO-8859-1 (Latin-1); the design extends to more by adding a table row.

# --- every string reports its encoding -------------------------------------
p "hello".encoding            # #<Encoding:UTF-8>   -- the default script encoding
p __ENCODING__                # #<Encoding:UTF-8>   -- ...which __ENCODING__ echoes

# --- the same bytes read two ways ------------------------------------------
euro = "€"               # the € sign
p euro.bytes                  # [226, 130, 172]  -- three UTF-8 bytes
p euro.bytesize               # 3   -- BYTES
p euro.length                 # 1   -- CHARACTERS
p euro.ascii_only?            # false
p "hello".ascii_only?         # true

# --- #b reinterprets the bytes as raw binary (ASCII-8BIT) ------------------
raw = euro.b
p raw.encoding                # #<Encoding:BINARY (ASCII-8BIT)>
p raw.bytesize                # 3
p raw.length                  # 3   -- now every byte is its own "character"

# --- force_encoding re-tags WITHOUT changing bytes -------------------------
# Reading UTF-8's multi-byte € as US-ASCII leaves the bytes invalid.
mislabeled = euro.dup.force_encoding("US-ASCII")
p mislabeled.encoding         # #<Encoding:US-ASCII>
p mislabeled.valid_encoding?  # false  -- 0x80+ bytes aren't valid US-ASCII

# --- the Encoding class and its constants ----------------------------------
p Encoding::UTF_8             # #<Encoding:UTF-8>
p Encoding::UTF_8.name        # "UTF-8"
p Encoding::ASCII_8BIT.names  # ["ASCII-8BIT", "BINARY"]
p Encoding.default_external   # #<Encoding:UTF-8>
p Encoding.find("BINARY")     # #<Encoding:BINARY (ASCII-8BIT)>  -- alias lookup
# An ASCII-only string is compatible with any ascii-compatible encoding.
p Encoding.compatible?("plain ascii", euro)   # #<Encoding:UTF-8>

# --- encode TRANSCODES the bytes between encodings -------------------------
accented = "café"        # "café" in UTF-8 (é is two bytes)
latin1 = accented.encode(Encoding::ISO_8859_1)
p latin1.encoding             # #<Encoding:ISO-8859-1>
p latin1.bytes                # [99, 97, 102, 233]  -- é is now ONE Latin-1 byte
p latin1.encode(Encoding::UTF_8) == accented   # true  -- round trip

# A character with no target representation raises...
begin
  accented.encode(Encoding::US_ASCII)
rescue Encoding::UndefinedConversionError => e
  puts "raised: #{e.class}"
end
# ...unless you ask for replacement.
p accented.encode(Encoding::US_ASCII, undef: :replace)          # "caf?"
p 'a<b>&c'.encode(Encoding::US_ASCII, xml: :text)               # "a&lt;b&gt;&amp;c"

# --- Symbols and Regexps carry an encoding too -----------------------------
p :hello.encoding             # #<Encoding:US-ASCII>  -- all-ASCII default
p :café.encoding              # #<Encoding:UTF-8>
p(/ab+/.encoding)             # #<Encoding:US-ASCII>
__END__
#<Encoding:UTF-8>
#<Encoding:UTF-8>
[226, 130, 172]
3
1
false
true
#<Encoding:BINARY (ASCII-8BIT)>
3
3
#<Encoding:US-ASCII>
false
#<Encoding:UTF-8>
"UTF-8"
["ASCII-8BIT", "BINARY"]
#<Encoding:UTF-8>
#<Encoding:BINARY (ASCII-8BIT)>
#<Encoding:UTF-8>
#<Encoding:ISO-8859-1>
[99, 97, 102, 233]
true
raised: Encoding::UndefinedConversionError
"caf?"
"a&lt;b&gt;&amp;c"
#<Encoding:US-ASCII>
#<Encoding:UTF-8>
#<Encoding:US-ASCII>

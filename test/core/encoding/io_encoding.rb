# Reading a file yields a string tagged with an ENCODING. A text read
# (File.read) tags the bytes with the EXTERNAL encoding (UTF-8 by default,
# or one you request) without validating them; a binary read (File.binread)
# always yields raw ASCII-8BIT bytes. Writes emit the bytes verbatim.

require "tmpdir"   # Dir.mktmpdir -- a built-in feature here (a no-op require)

Dir.mktmpdir do |dir|
  path = File.join(dir, "note.txt")
  File.write(path, "café")   # 5 bytes: c a f + the two UTF-8 bytes of é

  # --- text read: tagged with the external encoding -------------------------
  text = File.read(path)
  p text.encoding            # #<Encoding:UTF-8>   -- the default external
  p text.bytesize            # 5

  # --- binary read: always ASCII-8BIT, no interpretation --------------------
  raw = File.binread(path)
  p raw.encoding             # #<Encoding:BINARY (ASCII-8BIT)>
  p raw.bytes                # [99, 97, 102, 195, 169]

  # --- request a specific external encoding ---------------------------------
  latin = File.read(path, encoding: "ISO-8859-1")
  p latin.encoding           # #<Encoding:ISO-8859-1>  -- same bytes, retagged
  p latin.bytes              # [99, 97, 102, 195, 169]

  # ...and an internal encoding TRANSCODES on the way in.
  utf = File.read(path, external_encoding: "ISO-8859-1", internal_encoding: "UTF-8")
  p utf.encoding             # #<Encoding:UTF-8>
  p utf.bytesize             # 7  -- the two Latin-1 high bytes became UTF-8

  # --- binwrite round-trips arbitrary bytes ---------------------------------
  File.binwrite(path, [0, 255, 128, 10].pack("C*"))
  p File.binread(path).bytes # [0, 255, 128, 10]
end
__END__
#<Encoding:UTF-8>
5
#<Encoding:BINARY (ASCII-8BIT)>
[99, 97, 102, 195, 169]
#<Encoding:ISO-8859-1>
[99, 97, 102, 195, 169]
#<Encoding:UTF-8>
7
[0, 255, 128, 10]

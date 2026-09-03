# encoding: ISO-8859-1
# A `# encoding:` magic comment on the first line (or the second, after a
# shebang) sets the SCRIPT ENCODING: the encoding every string literal in the
# file is tagged with, and the one `__ENCODING__` reports. The default, with
# no such comment, is UTF-8.

# `__ENCODING__` echoes the script encoding declared above.
p __ENCODING__            # #<Encoding:ISO-8859-1>

# Every string literal is born in that encoding...
s = "hello"
p s.encoding              # #<Encoding:ISO-8859-1>
p s.bytes                 # [104, 101, 108, 108, 111]  -- ASCII bytes, Latin-1 tag

# ...so it is Latin-1-compatible, not UTF-8.
p s.encoding == Encoding::ISO_8859_1   # true
p s.force_encoding("UTF-8").encoding   # #<Encoding:UTF-8>  -- retag if you want

# A literal can still be transcoded like any other string.
p "café".encode(Encoding::UTF_8).encoding   # #<Encoding:UTF-8>
__END__
#<Encoding:ISO-8859-1>
#<Encoding:ISO-8859-1>
[104, 101, 108, 108, 111]
true
#<Encoding:UTF-8>
#<Encoding:UTF-8>

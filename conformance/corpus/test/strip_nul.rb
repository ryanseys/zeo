# strip/lstrip/rstrip remove the NUL byte (CRuby strips "\0\t\n\v\f\r ").
# Embedded NUL needs a heap string (literals truncate at \0); build via pack.
s = [0, 104, 105, 0].pack("C*")          # "\0hi\0"
p s.strip.bytes
p s.lstrip.bytes
p s.rstrip.bytes
mid = [0, 0, 104, 0, 105, 0].pack("C*")  # "\0\0h\0i\0" -- interior NUL survives
p mid.strip.bytes
# normal strings unchanged
p "  hi  ".strip
p "\tx\n".strip
p "ab  ".rstrip
p "  ab".lstrip

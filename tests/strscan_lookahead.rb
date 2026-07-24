require "strscan"

s = StringScanner.new("test string")

# exist? looks ahead without advancing: bytes from here to the match end.
p s.exist?(/s/)
p s.pos

# check_until peeks up to and including the next match (no advance).
p s.check_until(/str/)
p s.pos

# get_byte reads a single byte and advances by one.
p s.get_byte
p s.get_byte
p s.pos

# unscan backs up over the most recent scan.
tok = s.scan(/st /)
p tok
p s.pos
s.unscan
p s.pos
p s.scan(/st /)

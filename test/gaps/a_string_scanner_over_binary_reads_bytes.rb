# A StringScanner over a binary String reads its high bytes as bytes: they are
# not letters, so `[[:alpha:]]` and `\b` see none there.
require "strscan"
s = StringScanner.new("\xB5a".b)
p s.scan(/[[:alpha:]]/)
p s.scan_until(/\b/).bytes
p s.pos
__END__
nil
[181]
1

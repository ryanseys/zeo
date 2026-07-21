# Range#=== (and cover?/include?) on a String range with a non-String argument
# returns false, like CRuby, rather than feeding a non-pointer to strcmp (which
# segfaulted). String-in-string-range and numeric ranges are unaffected.
p(('A'..'C') === 20)
p(('A'..'C') === 'B')
p(('A'..'C') === 'Z')
p(('a'..'z') === 'm')
p(('a'..'z').cover?(5))
p((1..5) === 3)
p((1..5) === 9)

# The POSIX bracket class `[[:alpha:]]` (and presumably its siblings) only
# matches ASCII letters in zeo's regex engine -- CRuby's onig engine matches
# any Unicode letter (e.g. "e" with an accent) against `[[:alpha:]]`.
p "café".match?(/\A[[:alpha:]]+\z/)
p "abc".match?(/\A[[:alpha:]]+\z/)
p "é".match?(/[[:alpha:]]/)

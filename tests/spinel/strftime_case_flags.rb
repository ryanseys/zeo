# `%#` changes a field's case as a whole -- all-uppercase becomes lowercase,
# anything else becomes uppercase -- rather than swapping it per character.
# Per-character swapcase turned "January" into "jANUARY" where CRuby answers
# "JANUARY"; only fields that are already uppercase (%p, %Z) agreed. Found
# when a clang build refused lib/sp_time.c for calling tolower with no
# <ctype.h>, which made the implicit int return the thing to look at.
t = Time.at(0).utc
p t.strftime("%#B")
p t.strftime("%#b")
p t.strftime("%#A")
p t.strftime("%#a")
p t.strftime("%#p")
p t.strftime("%#P")
p t.strftime("%#Z")
p t.strftime("%#j")
p t.strftime("%^a")
p t.strftime("%^B")

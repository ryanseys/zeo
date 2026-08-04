# Everything after a line reading `__END__` is the program's DATA section, and
# ruby exposes it as an open `File` in the `DATA` constant. zeo defines no
# `DATA`, so `defined?(DATA)` is nil and the text after `__END__` is simply
# dropped.
#
# It is the oldest way ruby scripts carry their own fixture -- a template, a
# word list, a test corpus -- in one file with no second path to resolve, and
# it is what makes a single-file script self-contained. `DATA` is an ordinary
# IO positioned just past the marker, so `read`/`each_line`/`rewind` all work
# on it.
#
# zeo's lexer already stops at `__END__` (the program below runs, and nothing
# after the marker is parsed as code); what is missing is keeping the bytes and
# opening the constant over them.
#
# (`DATA.rewind` seeks to offset 0 of the SOURCE file, not to the marker, so it
# is deliberately not exercised here -- the golden would carry this comment.)

p defined?(DATA)
p DATA.class
p DATA.read
__END__
first line
second line

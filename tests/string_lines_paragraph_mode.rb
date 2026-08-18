# GAP -- imported from the spinel corpus at c55d9bdb.
# String#lines in paragraph mode (a "" separator) does not split on the
# blank line.
#
p "a\n\nb\n".lines("")
r004 = ("1-2-3".lines("-", chomp: true) rescue $!.class); p r004
p "1-2-3".lines("-")
p "a\nb\n".lines(chomp: true)
p "a\n\n\nb".lines("")
p "".lines("")
p "a\n\n\n\nb".lines("")
p "a\n\n\nb\n\n\n".lines("")
p "a\nb".lines("")
p "\n\na".lines("")

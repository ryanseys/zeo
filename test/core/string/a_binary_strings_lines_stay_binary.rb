# A binary String's lines are binary Strings holding the same bytes.
p "\xE1\n\xE2".b.lines.map { [_1.bytes, _1.encoding] }
p "\xE1\n".b.each_line.to_a.map(&:bytes)
__END__
[[[225, 10], #<Encoding:BINARY (ASCII-8BIT)>], [[226], #<Encoding:BINARY (ASCII-8BIT)>]]
[[225, 10]]

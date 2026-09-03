# GAP -- imported from the spinel corpus at c55d9bdb.
# The missing-key KeyError message brackets the key with <> where ruby uses
# {}.
#
# A missing key in a `%{name}` / `%<name>s` format is CRuby's KeyError, not a
# nil that rendered as the empty string, and a String argument to an integer
# conversion goes through Integer()'s parse, so unparseable text raises rather
# than reading as zero.
p(("%d" % "x" rescue $!.class))
p(("%{z}" % { a: 1 } rescue $!.class))
p(("%<z>d" % { a: 1 } rescue $!.class))
r = (format("%{z}", a: 1) rescue $!.class); p r
p("%d" % "12")
p("%{a}" % { a: 1 })
p(("%x" % "zz" rescue $!.class))
p("%x" % "255")
p("%<a>d" % { a: 5 })
p("%{a}-%{b}" % { a: 1, b: 2 })
p(("%{zz}" % { a: 1 } rescue $!.message))
p(("%<zz>d" % { a: 1 } rescue $!.message))
__END__
ArgumentError
KeyError
KeyError
KeyError
"12"
"1"
ArgumentError
"ff"
"5"
"1-2"
"key{zz} not found"
"key<zz> not found"

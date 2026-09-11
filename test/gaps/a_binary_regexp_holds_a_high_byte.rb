# A binary pattern keeps its bytes, and ASCII-8BIT with them. `\M-a` and a
# raw high byte both match a binary string, both report ASCII-8BIT, and the
# source stays the four bytes as written. (No `encoding:` on line 1: that
# spelling is a magic comment, and ruby refuses the file.) `//n` is US-ASCII, not binary,
# because the flag narrows rather than reinterprets.
def t(l); r=(begin; yield.inspect; rescue Exception=>e; "#{e.class}: #{e.message}"; end); puts format("%-24s %s", l, r); end
t("M-a in binary")     { Regexp.new("\\M-a".b) =~ "\xE1".b }
t("M-a binary enc")    { Regexp.new("\\M-a".b).encoding.to_s }
t("M-a binary source") { Regexp.new("\\M-a".b).source.bytes }
t("a raw high byte")   { Regexp.new("\xE1".b) =~ "\xE1".b }
t("raw byte enc")      { Regexp.new("\xE1".b).encoding.to_s }
t("n flag")            { //n.encoding.to_s }
__END__
M-a in binary            0
M-a binary enc           "ASCII-8BIT"
M-a binary source        [92, 77, 45, 97]
a raw high byte          0
raw byte enc             "ASCII-8BIT"
n flag                   "US-ASCII"

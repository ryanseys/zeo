# named_captures collects ALL indices for a name reused by several groups
# (the engine collapses it, so this parses the source); \+ expands to the
# highest participating group; Regexp#=~ against a non-String raises
# TypeError (nil still answers nil).

p /(?<a>x)(?<b>y)/.named_captures
p /(?<a>x)(?<a>z)/.named_captures
p /(?<a>x)(?<a>z)/.names
p "ab".sub(/(a)(b)?/, '\+')
p "a.".sub(/(a)(b)?/, '\+')
p "1a 2. 3c".gsub(/(\d)([a-z])?/, '<\+>')
r = (begin; /p/ =~ 5; rescue TypeError => e; "TE: " + e.message; end); p r
p(/l/ =~ "hello")
p(/z/ =~ "hello")
__END__
{"a" => [1], "b" => [2]}
{"a" => [1, 2]}
["a"]
"b"
"a."
"<a> <2>. <c>"
"TE: no implicit conversion of Integer into String"
2
nil

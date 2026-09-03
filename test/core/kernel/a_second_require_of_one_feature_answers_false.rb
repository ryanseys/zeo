p require("json")
p require("json")

# Already loaded before line 1, so even the FIRST require answers false.
p require("set")
p require("set")

# Two requires inside one statement: the first loads, the second does not.
p [require("digest"), require("digest")]

# A gem with a Ruby half on disk answers the same way, and the require makes
# its constant resolvable at the same time.
loaded = require("csv")
p [loaded, defined?(CSV)]
p require("csv")
__END__
true
false
false
false
[true, false]
[true, "constant"]
false

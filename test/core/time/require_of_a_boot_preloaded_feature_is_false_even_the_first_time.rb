# `set` and `monitor` are already loaded before a CRuby program's first
# line, so even their FIRST require answers false.

p(require "set")
p(require "monitor")
__END__
false
false

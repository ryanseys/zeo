# rexml's `class Output; include Encoding` means REXML::Encoding, the gem's
# own module -- never the builtin Encoding CLASS an outer scope offers. The
# resolution must hold when rexml arrives as a lazy unit too, which is how
# `require "rss"` compiles it.
require "rexml/document"

p REXML::Output.ancestors.first(3)
doc = REXML::Document.new("<a><b>text</b></a>")
p doc.root.name
p doc.root.elements["b"].text
__END__
[REXML::Output, REXML::Encoding, Object]
"a"
"text"

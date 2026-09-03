# rexml's `class Output; include Encoding` means REXML::Encoding, the gem's
# own module -- never the builtin Encoding CLASS an outer scope offers. The
# resolution must hold when rexml arrives as a lazy unit too, which is how
# `require "rss"` compiles it.
#
# A parsed document is a parent/child ring by construction: every element
# holds its children and each child holds its parent. rexml never breaks it,
# so the tree this program parses is alive at exit.
#@ gccheck: cycle leak: 17 objects (Hash x4, Array x3, REXML::Attributes x3, REXML::Elements x3, REXML::Element x2, REXML::Document x1, REXML::Text x1)
require "rexml/document"

p REXML::Output.ancestors.first(3)
doc = REXML::Document.new("<a><b>text</b></a>")
p doc.root.name
p doc.root.elements["b"].text
__END__
[REXML::Output, REXML::Encoding, Object]
"a"
"text"

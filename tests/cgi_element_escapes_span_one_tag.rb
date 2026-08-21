# `CGI.escapeElement` / `CGI.unescapeElement` rewrite only the tags they were
# asked about, and each span ends at the next DELIMITER.
#
# CRuby writes each as a regexp, and the two differ only in the alphabet
# (`cgi/escape.rb`, ruby 4.0.6):
#
#   escapeElement    /<\/?(?:A|B)\b[^<>]*+>?/im
#   unescapeElement  /&lt;\/?(?:A|B)\b(?>[^&]+|&(?![gl]t;)\w+;)*(?:&gt;)?/im
#
# Both read the tag name, then run to the next open or close delimiter, and
# take the close only when that is what stopped them. So an unterminated tag
# still matches, and a tag whose attributes carry an escaped delimiter ends
# AT it rather than running on to the next close.
#
# The escaped form also stops at any `&` that does not begin a `&word;`
# entity, which is why a NUMERIC reference ends a span: `&#62;` has no word
# character after the `&`.
require "cgi"

# The documented shapes.
p CGI.escapeElement('<BR><A HREF="url"></A>', "A", "IMG")
p CGI.escapeElement('<BR><A HREF="url"></A>', ["A", "IMG"])
p CGI.unescapeElement(CGI.escapeHTML('<BR><A HREF="url"></A>'), "A", "IMG")
p CGI.unescapeElement(CGI.escapeHTML('<BR><A HREF="url"></A>'), ["A", "IMG"])

# An UNTERMINATED tag matches: the close is optional.
p CGI.escapeElement("<A", "A")
p CGI.unescapeElement("&lt;A", "A")
p CGI.unescapeElement("&lt;/A", "A")

# A delimiter inside the attributes ENDS the span.
p CGI.escapeElement('<A HREF="a<b">t</A>', "A")
p CGI.unescapeElement("&lt;A HREF=&quot;a&lt;b&quot;&gt;t&lt;/A&gt;", "A")

# A numeric reference ends an escaped span; a named one does not.
p CGI.unescapeElement("&lt;A ALT=&#62;&gt;t&lt;/A&gt;", "A")
p CGI.unescapeElement("&lt;A ALT=&quot;q&quot;&gt;t&lt;/A&gt;", "A")
p CGI.unescapeElement("&lt;A x=&nosemi&gt;", "A")

# The name is read to its end, so a longer tag is a different tag.
p CGI.escapeElement("<ABBR>x</ABBR>", "A")
p CGI.unescapeElement("&lt;AB&gt;t&lt;/AB&gt;", "A")

# Matching is case-insensitive, and an empty element list rewrites nothing.
p CGI.escapeElement("<a href=x>", "A")
p CGI.escapeElement("<A>", [])
p CGI.unescapeElement("&lt;A&gt;", [])

# Only the named tags move.
p CGI.escapeElement("<A>t</A><B>u</B>", "B")
p CGI.unescapeElement("&lt;A&gt;t&lt;/A&gt;&lt;B&gt;u&lt;/B&gt;", "B")

# The element forms leave the bytes around a span alone, invalid ones too.
doc = "&lt;A&gt;t&lt;/A&gt;\xC0".dup.force_encoding("BINARY")
r = CGI.unescapeElement(doc, "A")
puts "#{r.inspect} #{r.encoding}"

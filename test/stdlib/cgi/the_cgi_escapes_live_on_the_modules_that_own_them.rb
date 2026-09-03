# `CGI.escapeHTML` is not a class method of `CGI`. It is an instance method of
# `CGI::EscapeExt`, reached because `CGI` both INCLUDES and EXTENDS
# `CGI::Escape`, and `Escape` PREPENDS `EscapeExt`. zeo hung the helpers
# straight on CGI's singleton and had neither module, so erubi's
# `defined?(::CGI::Escape)` and net-http-persistent's `defined?(CGI::EscapeExt)`
# both answered nil and took their fallback branch.
require "cgi/escape"

p [CGI::Escape.class, CGI::EscapeExt.class]
p CGI::Escape.name, CGI::EscapeExt.name

# Four edges, none derivable from the others.
p CGI.ancestors.take(4)
p CGI.singleton_class.ancestors.take(3)
p CGI::Escape.ancestors
p CGI::EscapeExt.ancestors

# The two overlap on eight names and differ on the rest: only `Escape` has the
# `*Element` family, only `EscapeExt` has `h` and the snake spellings.
p CGI::Escape.instance_methods(false).sort
p CGI::EscapeExt.instance_methods(false).sort

p CGI.escapeHTML("<a href='x'>&\"")
p CGI.unescapeHTML("&lt;a&gt;&amp;&quot;&#39;")
p CGI.escape("a b+c/d")
p CGI.unescape("a+b%2Fc")
p CGI.escapeURIComponent("a b/c")
p CGI.unescapeURIComponent("a+b%2Fc")
p CGI.escape_html("<x>")
p CGI.unescape_html("&lt;x&gt;")
p CGI.escape_uri_component("a b")
p CGI.unescape_uri_component("a%20b")
p CGI.h("<x>")

# `escapeElement` escapes the TAGS of the elements it was named, and nothing
# else -- not the text between them, and not a tag it was not asked about.
p CGI.escapeElement("<A HREF='x'>t</A><B>b</B>", "A")
p CGI.escapeElement("<A><B>", "A", "B")
p CGI.escapeElement("<A><B>", ["A"])
p CGI.escapeElement("<A>")
# A longer tag is not the one named.
p CGI.escapeElement("<ABBR><A>", "A")
p CGI.unescapeElement("&lt;A&gt;&lt;B&gt;", "A")
p CGI.escape_element("<A><B>", "A")
p CGI.unescape_element("&lt;A&gt;&lt;B&gt;", "A")

# Mixing the module in is what the gems that reach for it actually do.
class Own
  include CGI::Escape
end
p Own.new.escapeHTML("<a>")
p Own.new.escapeElement("<A>", "A")
p Own.ancestors.take(4)
p Own.new.is_a?(CGI::EscapeExt)
p Own.instance_method(:escapeHTML).owner
__END__
[Module, Module]
"CGI::Escape"
"CGI::EscapeExt"
[CGI, CGI::EscapeExt, CGI::Escape, Object]
[#<Class:CGI>, CGI::EscapeExt, CGI::Escape]
[CGI::EscapeExt, CGI::Escape]
[CGI::EscapeExt]
[:escape, :escapeElement, :escapeHTML, :escapeURIComponent, :escape_element, :escape_uri_component, :unescape, :unescapeElement, :unescapeHTML, :unescapeURIComponent, :unescape_element, :unescape_uri_component]
[:escape, :escapeHTML, :escapeURIComponent, :escape_html, :escape_uri_component, :h, :unescape, :unescapeHTML, :unescapeURIComponent, :unescape_html, :unescape_uri_component]
"&lt;a href=&#39;x&#39;&gt;&amp;&quot;"
"<a>&\"'"
"a+b%2Bc%2Fd"
"a b/c"
"a%20b%2Fc"
"a+b/c"
"&lt;x&gt;"
"<x>"
"a%20b"
"a b"
"&lt;x&gt;"
"&lt;A HREF=&#39;x&#39;&gt;t&lt;/A&gt;<B>b</B>"
"&lt;A&gt;&lt;B&gt;"
"&lt;A&gt;<B>"
"<A>"
"<ABBR>&lt;A&gt;"
"<A>&lt;B&gt;"
"&lt;A&gt;<B>"
"<A>&lt;B&gt;"
"&lt;a&gt;"
"&lt;A&gt;"
[Own, CGI::EscapeExt, CGI::Escape, Object]
true
CGI::EscapeExt

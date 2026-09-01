require "erb"

# ERB::Util's html_escape comes from ERB::Escape -- CRuby's C erb/escape
# extension, zeo's spliced shim over CGI.escapeHTML. Same answers either way.
p ERB::Util.html_escape(%(<a href="x">R&D 'lab'</a>))
p ERB::Escape.html_escape("1 < 2 & 3 > 2")
p ERB::Util.html_escape(nil)
p ERB::Util.html_escape(:sym)
p ERB::Util.url_encode("a b/c?d=e&f")

# The feature is loadable by name, and only once.
p require "erb/escape"

# And the template road uses it end to end.
p ERB.new(%(<%= ERB::Util.html_escape(x) %>)).result_with_hash(x: "<i>&</i>")

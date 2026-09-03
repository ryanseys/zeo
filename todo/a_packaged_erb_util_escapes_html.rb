# The spliced road matches ruby; the PACKAGED road (compiled against
# packaged gems rather than spliced sources) is the one that diverges.
require "erb"
puts ERB::Util.html_escape("<a & b>")
__END__
&lt;a &amp; b&gt;

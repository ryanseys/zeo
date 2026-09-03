#@ pkggap
require "erb"
puts ERB::Util.html_escape("<a & b>")
__END__
&lt;a &amp; b&gt;

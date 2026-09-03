require "cgi/escape"
puts CGI.escape("a b&c=d")
puts CGI.escapeHTML("<x>&'")
puts CGI.unescape("a+b%26c")
__END__
a+b%26c%3Dd
&lt;x&gt;&amp;&#39;
a b&c

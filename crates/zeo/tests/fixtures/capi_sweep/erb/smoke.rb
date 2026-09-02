require "erb/escape"

puts ERB::Escape.html_escape(%q{<a href="x">Tom & Jerry's</a>})
require "erb"
puts ERB::Util.html_escape("1 < 2"), ERB.new("x=<%= 1 + 1 %>").result
puts ERB::Util.url_encode("a b&c")

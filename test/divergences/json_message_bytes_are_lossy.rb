# DECIDED DIVERGENCE, display only. When a parse error quotes the offending
# input back and that input is not valid UTF-8, ruby puts the RAW BYTE in
# the message -- so the message itself is not valid UTF-8. Zeo builds its
# messages as Rust `String`s, which cannot hold one, so the byte renders as
# U+FFFD.
#
# The class, the position and the shape of the message all agree; only the
# byte differs, and only inside the quoted echo of input the program
# already has. `test/stdlib/json/json_parser_edge_cases.rb` holds every other row of
# that family, all of them exact.

require "json"

begin
  JSON.parse("[\xFF]".b)
rescue JSON::ParserError => e
  puts e.class
  puts e.message.bytes.inspect
end
__END__
JSON::ParserError
[117, 110, 101, 120, 112, 101, 99, 116, 101, 100, 32, 99, 104, 97, 114, 97, 99, 116, 101, 114, 58, 32, 39, 239, 191, 189, 93, 39, 32, 97, 116, 32, 108, 105, 110, 101, 32, 49, 32, 99, 111, 108, 117, 109, 110, 32, 50]

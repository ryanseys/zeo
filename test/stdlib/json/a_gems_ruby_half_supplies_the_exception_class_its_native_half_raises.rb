# A gem's Ruby half can reopen its feature-gated NATIVE half and nest an
# exception class inside it, which the native half then raises by name.
# This is what retires the `StringScanner::Error` -> `RuntimeError`
# divergence: a nested user exception registers under its fully qualified
# name with a real constructor, where an ABI row cannot.

require "json"
require "strscan"
begin
  JSON.parse("{oops")
rescue JSON::ParserError => e
  p e.class.name
  p e.class.ancestors.include?(StandardError)
end
p JSON::ParserError.superclass.name
p StringScanner::Error.ancestors.include?(StandardError)
__END__
"JSON::ParserError"
true
"JSON::JSONError"
true

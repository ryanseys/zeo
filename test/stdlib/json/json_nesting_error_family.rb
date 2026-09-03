# JSON's depth control: too-deep input raises JSON::NestingError (a
# distinct class rescue code depends on), and `max_nesting:` bounds the
# parse. zeo raises ParserError with serde's text for the first and
# ignores the option outright for the second. The parser MESSAGE texts
# are the decided serde_json substitution; the missing CLASS and the
# ignored OPTION are not. (Found by the 2026-08-24 probe sweep.)
require "json"
begin
  JSON.parse("[" * 200 + "]" * 200)
rescue JSON::NestingError => e
  puts "#{e.class}: #{e.message}"
end
begin
  p JSON.parse("[[[1]]]", max_nesting: 2)
rescue JSON::NestingError => e
  puts "#{e.class}: #{e.message}"
end
__END__
JSON::NestingError: nesting of 101 is too deep
JSON::NestingError: nesting of 3 is too deep

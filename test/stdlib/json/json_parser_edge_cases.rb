# Every hostile shape the JSON parser has to survive, as ANSWERS rather than
# as a promise. The point of the file is that none of these ends the process:
# each row is either a value or a Ruby exception, and the golden pins which.
#
# Written after the parser was rewritten off serde_json
# (`crates/zeo-rt/ext/json/ext/json/src/parser.rs`), and it found real defects rather
# than confirming a design -- see `json_parser_stress.rb` for the generated
# half.
#
# ONE ROW IS A DECIDED DIVERGENCE and it is not here: a document nested past
# 10,000 deep. Ruby's parser keeps its own stack and reads a million-deep
# document with `max_nesting: false`; this one recurses, so it refuses past
# a ceiling far short of the frames the stack holds. A loud NestingError
# beats ending the process. `tests/json_nesting_is_bounded_by_the_stack.rb`
# records it.

require "json"

def show(name)
  r = yield
  puts "#{name}\t#{r.inspect}"
rescue Exception => e
  puts "#{name}\t#{e.class}: #{e.message}"
end

# --- Truncation, at every position a document has ------------------------
[
  "{", "}", "[", "]", "\"", ",", ":", "{\"", "{\"a", "{\"a\"", "{\"a\":",
  "{\"a\":1", "[1", "[1,", "[\"", "\"\\\\", "\"\\\\u", "\"\\\\u0",
  "\"\\\\ud8", "-", "0.", "1e", "1e+", "1E-", "0x", "tr", "fal", "nul"
].each do |frag|
  show("truncated #{frag.inspect}") { JSON.parse(frag) }
end

# --- Empty and blank -----------------------------------------------------
show("empty") { JSON.parse("") }
show("space only") { JSON.parse("   \n\t ") }
show("comment only") { JSON.parse("// nothing\n") }
show("block comment only") { JSON.parse("/* nothing */") }
show("unterminated block comment") { JSON.parse("[1] /* x") }
show("nul byte") { JSON.parse("[1]\0") }
show("bom") { JSON.parse("﻿[1]") }

# --- Numbers, including the ones that overflow ---------------------------
show("huge exponent") { JSON.parse("[1e400]").first }
show("huge negative exponent") { JSON.parse("[1e-400]").first }
show("absurd exponent") { JSON.parse("[1e999999999]").first }
show("absurd negative exponent") { JSON.parse("[1e-999999999]").first }
show("exponent overflows i32") { JSON.parse("[1e99999999999999999999]").first }
show("long integer") { JSON.parse("[#{'9' * 400}]").first.to_s.size }
show("long fraction") { JSON.parse("[0.#{'1' * 400}]").first }
show("i64 min") { JSON.parse("[-9223372036854775808]").first }
show("i64 min minus one") { JSON.parse("[-9223372036854775809]").first }
show("i64 max plus one") { JSON.parse("[9223372036854775808]").first }
show("negative zero") { JSON.parse("[-0]").first }
show("negative zero float") { JSON.parse("[-0.0]").first }
show("leading zero") { JSON.parse("[01]") }
show("leading zero float") { JSON.parse("[01.5]") }
show("bare minus") { JSON.parse("[-]") }
show("bare dot") { JSON.parse("[.5]") }
show("trailing dot") { JSON.parse("[1.]") }
show("plus sign") { JSON.parse("[+1]") }
show("two dots") { JSON.parse("[1.2.3]") }
show("hex") { JSON.parse("[0x10]") }
show("infinity word") { JSON.parse("[Infinity]") }
show("nan word") { JSON.parse("[NaN]") }
show("negative infinity") { JSON.parse("[-Infinity]") }

# --- Strings and escapes -------------------------------------------------
show("bad escape") { JSON.parse('"\\q"') }
show("short unicode escape") { JSON.parse('"\\u12"') }
show("non-hex unicode escape") { JSON.parse('"\\uZZZZ"') }
show("lone high surrogate") { JSON.parse('"\\ud800"') }
show("lone low surrogate") { JSON.parse('"\\udc00"') }
show("reversed surrogates") { JSON.parse('"\\udc00\\ud800"') }
show("valid surrogate pair") { JSON.parse('"\\ud83d\\ude00"') }
show("high surrogate then escape") { JSON.parse('"\\ud800\\n"') }
show("escaped nul") { JSON.parse('"\\u0000"').bytes }
show("raw control byte") { JSON.parse(%Q{"\x01"}) }
show("escaped solidus") { JSON.parse('"\\/"') }

# What a message QUOTES BACK: from the offending byte to the first
# whitespace, capped at 32. Not one byte, and not the rest of the input --
# `[x]` reaches the `]` because nothing separates them, and the same `x` on
# its own line stops at the newline.
show("quote to the closer") { JSON.parse("[x]") }
show("quote stops at nl") { JSON.parse("[\nx\n]") }
show("quote stops at space") { JSON.parse("[\nx ]") }
show("quote stops at tab") { JSON.parse("[\nx\t]") }
show("quote caps at 32") { JSON.parse("[\n#{"x" * 50}\n]") }
show("long string") { JSON.parse(%Q{"#{'a' * 100_000}"}).size }
show("deep escapes") { JSON.parse(%Q{"#{'\\n' * 10_000}"}).size }

# --- Structure -----------------------------------------------------------
show("duplicate keys") { JSON.parse('{"a":1,"a":2}') }
show("non-string key") { JSON.parse("{1:2}") }
show("missing colon") { JSON.parse('{"a" 1}') }
show("missing value") { JSON.parse('{"a":}') }
show("double comma") { JSON.parse("[1,,2]") }
show("leading comma") { JSON.parse("[,1]") }
show("trailing comma off") { JSON.parse("[1,]") }
show("trailing comma on") { JSON.parse("[1,]", allow_trailing_comma: true) }
show("object trailing comma on") { JSON.parse('{"a":1,}', allow_trailing_comma: true) }
show("only a comma on") { JSON.parse("[,]", allow_trailing_comma: true) }
show("mismatched close") { JSON.parse("[1}") }
show("extra close") { JSON.parse("[1]]") }
show("two documents") { JSON.parse("[1][2]") }
show("empty containers") { JSON.parse("[[],{},[{}]]") }
show("deep but legal") { JSON.parse("[" * 99 + "]" * 99).flatten.size }
show("one past the default") { JSON.parse("[" * 101 + "]" * 101) }

# --- Options under stress ------------------------------------------------
show("max_nesting 0") { JSON.parse("[" * 150 + "]" * 150, max_nesting: 0).class }
show("max_nesting nil") { JSON.parse("[[1]]", max_nesting: nil).class }
show("max_nesting negative") { JSON.parse("[[1]]", max_nesting: -1).class }
show("symbolize deep") { JSON.parse('{"a":{"b":1}}', symbolize_names: true) }
show("freeze deep") do
  v = JSON.parse('{"a":["b"]}', freeze: true)
  [v.frozen?, v.keys.first.frozen?, v["a"].frozen?, v["a"].first.frozen?]
end
show("object_class refusing") { JSON.parse('{"a":1}', object_class: Integer) }
show("array_class refusing") { JSON.parse("[1]", array_class: Integer) }
show("decimal_class refusing") { JSON.parse("[1.5]", decimal_class: Integer) }

# --- Bytes that are not text --------------------------------------------
show("invalid utf-8") { JSON.parse("[\"\xFF\"]".b) }
show("truncated utf-8") { JSON.parse("[\"\xE3\x81\"]".b) }
# NOT here: a high byte OUTSIDE a string. Ruby quotes the raw byte back in
# the message, so the message itself is not valid UTF-8; zeo's message
# builder takes a Rust `String` and renders it U+FFFD. Display only, and
# `tests/json_message_bytes_are_lossy.rb` records it.

# --- The whole document is a scalar --------------------------------------
show("bare true") { JSON.parse("true") }
show("bare null") { JSON.parse("null") }
show("bare number") { JSON.parse("42") }
show("bare string") { JSON.parse('"x"') }
show("bare word") { JSON.parse("nope") }
__END__
truncated "{"	JSON::ParserError: expected object key, got EOF at line 1 column 2
truncated "}"	JSON::ParserError: unexpected character: '}' at line 1 column 1
truncated "["	JSON::ParserError: unexpected end of input at line 1 column 2
truncated "]"	JSON::ParserError: unexpected character: ']' at line 1 column 1
truncated "\""	JSON::ParserError: unexpected end of input, expected closing " at line 1 column 2
truncated ","	JSON::ParserError: unexpected character: ',' at line 1 column 1
truncated ":"	JSON::ParserError: unexpected character: ':' at line 1 column 1
truncated "{\""	JSON::ParserError: unexpected end of input, expected closing " at line 1 column 3
truncated "{\"a"	JSON::ParserError: unexpected end of input, expected closing " at line 1 column 4
truncated "{\"a\""	JSON::ParserError: expected ':' after object key at line 1 column 5
truncated "{\"a\":"	JSON::ParserError: unexpected end of input at line 1 column 6
truncated "{\"a\":1"	JSON::ParserError: expected ',' or '}' after object value, got: EOF at line 1 column 7
truncated "[1"	JSON::ParserError: expected ',' or ']' after array value at line 1 column 3
truncated "[1,"	JSON::ParserError: unexpected end of input at line 1 column 4
truncated "[\""	JSON::ParserError: unexpected end of input, expected closing " at line 1 column 3
truncated "\"\\\\"	JSON::ParserError: unexpected end of input, expected closing " at line 1 column 4
truncated "\"\\\\u"	JSON::ParserError: unexpected end of input, expected closing " at line 1 column 5
truncated "\"\\\\u0"	JSON::ParserError: unexpected end of input, expected closing " at line 1 column 6
truncated "\"\\\\ud8"	JSON::ParserError: unexpected end of input, expected closing " at line 1 column 7
truncated "-"	JSON::ParserError: invalid number: '-' at line 1 column 1
truncated "0."	JSON::ParserError: invalid number: '0.' at line 1 column 1
truncated "1e"	JSON::ParserError: invalid number: '1e' at line 1 column 1
truncated "1e+"	JSON::ParserError: invalid number: '1e+' at line 1 column 1
truncated "1E-"	JSON::ParserError: invalid number: '1E-' at line 1 column 1
truncated "0x"	JSON::ParserError: unexpected token at end of stream 'x' at line 1 column 2
truncated "tr"	JSON::ParserError: unexpected token 'tr' at line 1 column 1
truncated "fal"	JSON::ParserError: unexpected token 'fal' at line 1 column 1
truncated "nul"	JSON::ParserError: unexpected token 'nul' at line 1 column 1
empty	JSON::ParserError: unexpected end of input at line 1 column 1
space only	JSON::ParserError: unexpected end of input at line 2 column 3
comment only	JSON::ParserError: unexpected end of input at line 2 column 1
block comment only	JSON::ParserError: unexpected end of input at line 1 column 14
unterminated block comment	JSON::ParserError: unterminated comment, expected closing '*/' at line 1 column 5
nul byte	JSON::ParserError: unexpected token at end of stream  at line 1 column 4
bom	JSON::ParserError: unexpected character: '﻿[1]' at line 1 column 1
huge exponent	Infinity
huge negative exponent	0.0
absurd exponent	Infinity
absurd negative exponent	0.0
exponent overflows i32	Infinity
long integer	400
long fraction	0.1111111111111111
i64 min	-9223372036854775808
i64 min minus one	-9223372036854775809
i64 max plus one	9223372036854775808
negative zero	0
negative zero float	-0.0
leading zero	JSON::ParserError: invalid number: '01]' at line 1 column 2
leading zero float	JSON::ParserError: invalid number: '01.5]' at line 1 column 2
bare minus	JSON::ParserError: invalid number: '-]' at line 1 column 2
bare dot	JSON::ParserError: unexpected character: '.5]' at line 1 column 2
trailing dot	JSON::ParserError: invalid number: '1.]' at line 1 column 2
plus sign	JSON::ParserError: unexpected character: '+1]' at line 1 column 2
two dots	JSON::ParserError: expected ',' or ']' after array value at line 1 column 5
hex	JSON::ParserError: expected ',' or ']' after array value at line 1 column 3
infinity word	JSON::ParserError: unexpected token 'Infinity]' at line 1 column 2
nan word	JSON::ParserError: unexpected token 'NaN]' at line 1 column 2
negative infinity	JSON::ParserError: invalid number: '-Infinity]' at line 1 column 2
bad escape	JSON::ParserError: invalid escape character in string: '\q"' at line 1 column 2
short unicode escape	JSON::ParserError: incomplete unicode character escape sequence at '\u12"' at line 1 column 2
non-hex unicode escape	JSON::ParserError: incomplete unicode character escape sequence at '\uZZZZ"' at line 1 column 2
lone high surrogate	JSON::ParserError: incomplete surrogate pair at '\ud800"' at line 1 column 2
lone low surrogate	"\xED\xB0\x80"
reversed surrogates	JSON::ParserError: incomplete surrogate pair at '\ud800"' at line 1 column 8
valid surrogate pair	"😀"
high surrogate then escape	JSON::ParserError: incomplete surrogate pair at '\ud800\n"' at line 1 column 2
escaped nul	[0]
raw control byte	JSON::ParserError: invalid ASCII control character in string: '"' at line 1 column 2
escaped solidus	"/"
quote to the closer	JSON::ParserError: unexpected character: 'x]' at line 1 column 2
quote stops at nl	JSON::ParserError: unexpected character: 'x' at line 2 column 1
quote stops at space	JSON::ParserError: unexpected character: 'x' at line 2 column 1
quote stops at tab	JSON::ParserError: unexpected character: 'x' at line 2 column 1
quote caps at 32	JSON::ParserError: unexpected character: 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx' at line 2 column 1
long string	100000
deep escapes	10000
duplicate keys	{"a" => 2}
non-string key	JSON::ParserError: expected object key, got '1:2}' at line 1 column 2
missing colon	JSON::ParserError: expected ':' after object key at line 1 column 6
missing value	JSON::ParserError: unexpected character: '}' at line 1 column 6
double comma	JSON::ParserError: unexpected character: ',2]' at line 1 column 4
leading comma	JSON::ParserError: unexpected character: ',1]' at line 1 column 2
trailing comma off	JSON::ParserError: unexpected character: ']' at line 1 column 4
trailing comma on	[1]
object trailing comma on	{"a" => 1}
only a comma on	JSON::ParserError: unexpected character: ',]' at line 1 column 2
mismatched close	JSON::ParserError: expected ',' or ']' after array value at line 1 column 3
extra close	JSON::ParserError: unexpected token at end of stream ']' at line 1 column 4
two documents	JSON::ParserError: unexpected token at end of stream '[2]' at line 1 column 4
empty containers	[[], {}, [{}]]
deep but legal	0
one past the default	[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]
max_nesting 0	Array
max_nesting nil	Array
max_nesting negative	JSON::NestingError: nesting of 1 is too deep
symbolize deep	{a: {b: 1}}
freeze deep	[true, true, true, true]
object_class refusing	NoMethodError: undefined method 'new' for class Integer
array_class refusing	NoMethodError: undefined method 'new' for class Integer
decimal_class refusing	[nil]
invalid utf-8	["\xFF"]
truncated utf-8	["\xE3\x81"]
bare true	true
bare null	nil
bare number	42
bare string	"x"
bare word	JSON::ParserError: unexpected token 'nope' at line 1 column 1

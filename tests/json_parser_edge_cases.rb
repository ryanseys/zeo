# Every hostile shape the JSON parser has to survive, as ANSWERS rather than
# as a promise. The point of the file is that none of these ends the process:
# each row is either a value or a Ruby exception, and the golden pins which.
#
# Written after the parser was rewritten off serde_json
# (`crates/zeo-rt/src/ext/json/parser.rs`), and it found real defects rather
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

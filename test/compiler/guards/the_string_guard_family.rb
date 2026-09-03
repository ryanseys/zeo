# The String rows whose guard or option was remembered rather than
# structural, in one place.
#
# Four of them share a shape worth naming: the answer was computed over a
# TRANSFORMED copy of the receiver and then reported as if it were the
# original.
#
#   * `match(pattern, pos)` ran over a SLICE, so the MatchData reported
#     offsets relative to it -- `.begin(0)` said 1 where ruby says 4, and
#     `pre_match` lost everything before `pos`. The engine takes a start
#     offset directly, which is what CRuby uses.
#   * `scrub` TRANSCODED to its own encoding, which has nowhere to put a
#     block and re-encodes every valid character on the way. It now walks
#     the decoded spans and touches only the invalid runs.
#   * `encode` to an unknown encoding resolved each name as it read it, so
#     it reported an ArgumentError about ONE name where ruby reports a
#     converter failure naming the PAIR, source first, both raw as given.
#   * `newline:` as a SYMBOL was simply absent, so `encode(newline: :crlf)`
#     converted nothing. Its errors are asymmetric and copied that way: a
#     bad symbol is named, a non-symbol value is not.
#
# `force_encoding` is the fifth and simplest: it needed the frozen guard
# every other mutator has, BEFORE any comparison -- ruby refuses even when
# the new encoding equals the current one.

def show
  p yield
rescue Exception => e
  puts "#{e.class}: #{e.message}"
end
show { "a".freeze.force_encoding("BINARY") }
show { "a".freeze.force_encoding("UTF-8") }
show { (+"a").force_encoding("BINARY").encoding.to_s }
show { "hello world".match(/o/, 3)&.begin(0) }
show { "hello world".match(/o/, 5)&.begin(0) }
show { "hello world".match?(/o/, 5) }
show { "hello world".index("o", 5) }
show { "hello world".match(/(w)(o)/, 3)&.captures }
show { "hello world".match(/o/, 3)&.[](0) }
show { "hello world".match(/o/, 3)&.post_match }
show { "hello world".match(/o/, 3)&.pre_match }
show { "\xE3\x81\x82ab\xFFcd".dup.force_encoding("UTF-8").scrub { |b| "<#{b.bytes.map { |x| x.to_s(16) }.join}>" } }
show { "\xFF\xFEok".dup.force_encoding("UTF-8").scrub("?") }
show { (+"\xFFok").force_encoding("UTF-8").scrub! { |b| "!" } }
show { "a".encode("NoSuchEncoding") }
show { "a".encode("NoSuchTarget", "NoSuchSource") }
show { "a".encode("UTF-8", "NoSuchSource") }
show { Encoding.find("NoSuchEncoding") }
show { "a\r\nb".encode("UTF-8", newline: :universal) }
show { "a\nb".encode("UTF-8", newline: :crlf) }
show { "a\nb".encode("UTF-8", newline: :cr) }
show { "a\nb".encode("UTF-8", newline: :nope) }
show { "a\nb".encode("UTF-8", newline: 1) }
show { "a\nb".encode("UTF-8", crlf_newline: true) }
show { "a\nb".encode("UTF-8", cr_newline: true) }
show { "a\r\nb".encode("UTF-8", universal_newline: true) }
e = "\xC6\xFC".dup.force_encoding("EUC-JP")
show { e.casecmp("x") }
show { e.casecmp?("x") }
show { e =~ /x/ }
show { e.match?(/x/) }
show { e.match(/x/) }
show { e.index("x") }
show { e.gsub("x", "y") }
__END__
FrozenError: can't modify frozen String: "a"
FrozenError: can't modify frozen String: "a"
"ASCII-8BIT"
4
7
true
7
["w", "o"]
"o"
" world"
"hell"
"あab<ff>cd"
"??ok"
"!ok"
Encoding::ConverterNotFoundError: code converter not found (UTF-8 to NoSuchEncoding)
Encoding::ConverterNotFoundError: code converter not found (NoSuchSource to NoSuchTarget)
Encoding::ConverterNotFoundError: code converter not found (NoSuchSource to UTF-8)
ArgumentError: unknown encoding name - NoSuchEncoding
"a\nb"
"a\r\nb"
"a\rb"
ArgumentError: unexpected value for newline option: nope
ArgumentError: unexpected value for newline option
"a\r\nb"
"a\rb"
"a\nb"
1
false
nil
false
nil
nil
"\x{C6FC}"

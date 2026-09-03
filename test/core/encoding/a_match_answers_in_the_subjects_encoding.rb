# Every string a match hands back carries the SUBJECT's encoding, not the
# engine's. zeo decodes a haystack to UTF-8 to search it, and used to slice
# the groups out of that copy, so a binary or Latin-1 subject answered UTF-8
# for every one of them.
#
# The rows that already worked went through `String#match`, which threaded
# the encoding; the rest -- `Regexp#match`, `=~`, `$~`, `String#[]`,
# `partition` -- did not.

b = "café".b
l = "\xe9x".force_encoding("ISO-8859-1")

def show(label)
  puts format("%-34s %s", label, (yield).inspect)
rescue => e
  puts format("%-34s %s", label, "#{e.class}")
end

show("Regexp#match group")      { /ca(f)/.match(b)[1].encoding.to_s }
show("Regexp#match whole")      { /ca(f)/.match(b)[0].encoding.to_s }
show("Regexp#match pre")        { /a(f)/.match(b).pre_match.encoding.to_s }
show("Regexp#match post")       { /a(f)/.match(b).post_match.encoding.to_s }
show("Regexp#match latin1")     { /(.)/.match(l)[1].encoding.to_s }
show("String#match group")      { b.match(/ca(f)/)[1].encoding.to_s }
show("Regexp#=~ then $1")       { (/ca(f)/ =~ b; $1.encoding.to_s) }
show("String#=~ then $1")       { (b =~ /ca(f)/; $1.encoding.to_s) }
show("$~[0]")                   { (/ca(f)/ =~ b; $~[0].encoding.to_s) }
show("$~.pre_match")            { (/a(f)/ =~ b; $~.pre_match.encoding.to_s) }
show("String#scan")             { b.scan(/c(a)/).flatten.first.encoding.to_s }
show("String#[] regexp")        { b[/ca(f)/, 1].encoding.to_s }
show("String#slice regexp")     { b.slice(/caf/).encoding.to_s }
show("String#split")            { b.split(/f/).first.encoding.to_s }
show("gsub block match arg")    { b.gsub(/a/) { |m| break m.encoding.to_s } }
show("gsub block $1")           { b.gsub(/(a)/) { break $1.encoding.to_s } }
show("sub result")              { b.sub(/a/, "x").encoding.to_s }
show("String#match? no md")     { b.match?(/ca(f)/) }
show("named capture")           { /c(?<g>a)/.match(b)[:g].encoding.to_s }
show("named_captures")          { /c(?<g>a)/.match(b).named_captures["g"].encoding.to_s }
show("captures")                { /c(a)(f)/.match(b).captures.first.encoding.to_s }
show("values_at")               { /c(a)(f)/.match(b).values_at(1).first.encoding.to_s }
show("MatchData#string")        { /ca(f)/.match(b).string.encoding.to_s }
show("MatchData#to_a")          { /ca(f)/.match(b).to_a.first.encoding.to_s }
show("partition")               { b.partition(/a/)[1].encoding.to_s }
show("rpartition")              { b.rpartition(/a/)[1].encoding.to_s }
# A Symbol subject answers in the encoding the Symbol itself reports.
show("Symbol subject")          { /(b)/.match(:abc)[1].encoding.to_s }
__END__
Regexp#match group                 "ASCII-8BIT"
Regexp#match whole                 "ASCII-8BIT"
Regexp#match pre                   "ASCII-8BIT"
Regexp#match post                  "ASCII-8BIT"
Regexp#match latin1                "ISO-8859-1"
String#match group                 "ASCII-8BIT"
Regexp#=~ then $1                  "ASCII-8BIT"
String#=~ then $1                  "ASCII-8BIT"
$~[0]                              "ASCII-8BIT"
$~.pre_match                       "ASCII-8BIT"
String#scan                        "ASCII-8BIT"
String#[] regexp                   "ASCII-8BIT"
String#slice regexp                "ASCII-8BIT"
String#split                       "ASCII-8BIT"
gsub block match arg               "ASCII-8BIT"
gsub block $1                      "ASCII-8BIT"
sub result                         "ASCII-8BIT"
String#match? no md                true
named capture                      "ASCII-8BIT"
named_captures                     "ASCII-8BIT"
captures                           "ASCII-8BIT"
values_at                          "ASCII-8BIT"
MatchData#string                   "ASCII-8BIT"
MatchData#to_a                     "ASCII-8BIT"
partition                          "ASCII-8BIT"
rpartition                         "ASCII-8BIT"
Symbol subject                     "US-ASCII"

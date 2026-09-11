# A pattern built from a binary String, or written with /n, reads bytes. One
# holding a byte past 0x7f is pinned to ASCII-8BIT: it reports that encoding
# and FIXEDENCODING, keeps its bytes in #source and #to_s, and refuses a
# non-ASCII UTF-8 subject. An ASCII-only one pins nothing, and a /n one warns
# once when it meets a UTF-8 subject. A binary subject's high bytes are never
# word characters, whatever the pattern.
$stderr.sync = $stdout.sync = true
def t(label)
  r = begin; yield.inspect; rescue => e; "#{e.class}: #{e.message}"; end
  puts format("%-22s %s", label, r)
end
{
  'new("a".b)' => -> { Regexp.new("a".b) },
  'new("\xE1".b)' => -> { Regexp.new("\xE1".b) },
  'new("\\\\xE1".b)' => -> { Regexp.new("\\xE1".b) },
  'new("\\\\M-a".b)' => -> { Regexp.new("\\M-a".b) },
  '/[\x80-\xff]/n' => -> { /[\x80-\xff]/n },
  '//n' => -> { //n },
  'new("\\\\xE1", 32)' => -> { Regexp.new("\\xE1", 32) },
  'new("a", 16)' => -> { Regexp.new("a", 16) },
  'union("\xE9".b)' => -> { Regexp.union("\xE9".b) },
  'new("\xE1".b, "i")' => -> { Regexp.new("\xE1".b, "i") },
}.each do |label, make|
  re = make.()
  t("#{label} enc") { [re.encoding, re.options, re.fixed_encoding?] }
  t("#{label} text") { [re.inspect, re.to_s, re.source.encoding, re.source.bytes] }
  t("#{label} match") { ["\xE1".b, "\xC1".b, "a"].map { re =~ _1 } }
  t("#{label} utf8") { re =~ "é" }
end
t("new(é, 32)") { Regexp.new("é", 32) }
t("union é, E9") { Regexp.union("é", "\xE9".b) }
t("escape") { Regexp.escape("\xE1.".b).then { [_1.bytes, _1.encoding] } }
t("\\b \\w [[:word:]]") { ["\xB5".b =~ /\b/, "\xB5".b =~ /\w/, "\xB5".b =~ /[[:alpha:]]/] }
t("scan \\b") { "\xC2\xB5".b.scan(/\b/).size }
t("/i on a high byte") { "\xC9".b =~ Regexp.new("\xE9".b, Regexp::IGNORECASE) }
t("offsets") { "\xE9a".b.match(/a/).then { [_1.offset(0), _1.byteoffset(0), _1.pre_match.bytes] } }
t("byteindex") { ["\xE1x".b.byteindex(/x/), "\xE1x".b.byterindex(/x/), "éx".byteindex(/x/)] }
t("$` $'") { "\xE1x\xE2".b =~ /x/; [$`.bytes, $'.bytes, $~[0].encoding] }
t("sub gsub split") { ["\xE9".b.sub(/\xE9/n, "x"), "\xE1\xE1".b.gsub(Regexp.new("\xE1".b), "y"), "a\xE9b".b.split(/\xE9/n)] }
t("gsub block") { "a\xE9b\xE9".b.gsub(/\xE9/n) { $&.bytes.to_s } }
t("utf8 =~ bin") { "é" =~ /\xC3/n }
t("bin =~ utf8") { "\xE9".b =~ /é/ }
t("/n warns once") { r = //n; 3.times.map { r =~ "é" } }
t("utf8 \\x escapes") { %w[\\xFF \\xE9\\x41 \\xE9 \\351].map { Regexp.new(_1) rescue $!.message } }
t("utf8 \\x char") { Regexp.new("\\xC3\\xA9") =~ "é" }
__END__
new("a".b) enc         [#<Encoding:US-ASCII>, 0, false]
new("a".b) text        ["/a/", "(?-mix:a)", #<Encoding:US-ASCII>, [97]]
new("a".b) match       [nil, nil, 0]
new("a".b) utf8        nil
new("\xE1".b) enc      [#<Encoding:BINARY (ASCII-8BIT)>, 16, true]
new("\xE1".b) text     ["/\\xE1/", "(?-mix:\xE1)", #<Encoding:BINARY (ASCII-8BIT)>, [225]]
new("\xE1".b) match    [0, nil, nil]
new("\xE1".b) utf8     Encoding::CompatibilityError: incompatible encoding regexp match (BINARY (ASCII-8BIT) regexp with UTF-8 string)
new("\\xE1".b) enc     [#<Encoding:BINARY (ASCII-8BIT)>, 16, true]
new("\\xE1".b) text    ["/\\xE1/", "(?-mix:\\xE1)", #<Encoding:BINARY (ASCII-8BIT)>, [92, 120, 69, 49]]
new("\\xE1".b) match   [0, nil, nil]
new("\\xE1".b) utf8    Encoding::CompatibilityError: incompatible encoding regexp match (BINARY (ASCII-8BIT) regexp with UTF-8 string)
new("\\M-a".b) enc     [#<Encoding:BINARY (ASCII-8BIT)>, 16, true]
new("\\M-a".b) text    ["/\\M-a/", "(?-mix:\\M-a)", #<Encoding:BINARY (ASCII-8BIT)>, [92, 77, 45, 97]]
new("\\M-a".b) match   [0, nil, nil]
new("\\M-a".b) utf8    Encoding::CompatibilityError: incompatible encoding regexp match (BINARY (ASCII-8BIT) regexp with UTF-8 string)
/[\x80-\xff]/n enc     [#<Encoding:BINARY (ASCII-8BIT)>, 48, true]
/[\x80-\xff]/n text    ["/[\\x80-\\xff]/n", "(?-mix:[\\x80-\\xff])", #<Encoding:BINARY (ASCII-8BIT)>, [91, 92, 120, 56, 48, 45, 92, 120, 102, 102, 93]]
/[\x80-\xff]/n match   [0, 0, nil]
/[\x80-\xff]/n utf8    Encoding::CompatibilityError: incompatible encoding regexp match (BINARY (ASCII-8BIT) regexp with UTF-8 string)
//n enc                [#<Encoding:US-ASCII>, 32, false]
//n text               ["//n", "(?-mix:)", #<Encoding:US-ASCII>, []]
//n match              [0, 0, 0]
//n utf8               0
new("\\xE1", 32) enc   [#<Encoding:BINARY (ASCII-8BIT)>, 48, true]
new("\\xE1", 32) text  ["/\\xE1/n", "(?-mix:\\xE1)", #<Encoding:BINARY (ASCII-8BIT)>, [92, 120, 69, 49]]
new("\\xE1", 32) match [0, nil, nil]
new("\\xE1", 32) utf8  Encoding::CompatibilityError: incompatible encoding regexp match (BINARY (ASCII-8BIT) regexp with UTF-8 string)
new("a", 16) enc       [#<Encoding:UTF-8>, 16, true]
new("a", 16) text      ["/a/", "(?-mix:a)", #<Encoding:UTF-8>, [97]]
new("a", 16) match     Encoding::CompatibilityError: incompatible encoding regexp match (UTF-8 regexp with BINARY (ASCII-8BIT) string)
new("a", 16) utf8      nil
union("\xE9".b) enc    [#<Encoding:BINARY (ASCII-8BIT)>, 16, true]
union("\xE9".b) text   ["/\\xE9/", "(?-mix:\xE9)", #<Encoding:BINARY (ASCII-8BIT)>, [233]]
union("\xE9".b) match  [nil, nil, nil]
union("\xE9".b) utf8   Encoding::CompatibilityError: incompatible encoding regexp match (BINARY (ASCII-8BIT) regexp with UTF-8 string)
new("\xE1".b, "i") enc [#<Encoding:BINARY (ASCII-8BIT)>, 17, true]
new("\xE1".b, "i") text ["/\\xE1/i", "(?i-mx:\xE1)", #<Encoding:BINARY (ASCII-8BIT)>, [225]]
new("\xE1".b, "i") match [0, nil, nil]
new("\xE1".b, "i") utf8 Encoding::CompatibilityError: incompatible encoding regexp match (BINARY (ASCII-8BIT) regexp with UTF-8 string)
new(é, 32)             RegexpError: /.../n has a non escaped non ASCII character in non ASCII-8BIT script: /é/
union é, E9            ArgumentError: incompatible encodings: UTF-8 and ASCII-8BIT
escape                 [[225, 92, 46], #<Encoding:BINARY (ASCII-8BIT)>]
\b \w [[:word:]]       [nil, nil, nil]
scan \b                0
/i on a high byte      nil
offsets                [[1, 2], [1, 2], [233]]
byteindex              [1, 1, 2]
$` $'                  [[225], [226], #<Encoding:BINARY (ASCII-8BIT)>]
sub gsub split         ["x", "yy", ["a", "b"]]
gsub block             "a[233]b[233]"
utf8 =~ bin            Encoding::CompatibilityError: incompatible encoding regexp match (BINARY (ASCII-8BIT) regexp with UTF-8 string)
bin =~ utf8            Encoding::CompatibilityError: incompatible encoding regexp match (UTF-8 regexp with BINARY (ASCII-8BIT) string)
/n warns once          [0, 0, 0]
utf8 \x escapes        ["invalid multibyte escape: /\\xFF/", "invalid multibyte escape: /\\xE9\\x41/", "too short escaped multibyte character: /\\xE9/", "too short escaped multibyte character: /\\351/"]
utf8 \x char           0
#@ stderr
core/regexp/a_binary_pattern_reads_bytes.rb:43: warning: historical binary regexp match /.../n against UTF-8 string

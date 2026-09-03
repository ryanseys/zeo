# `RegexpError` messages, which CRuby words after onig's own reasons and
# echoes the pattern into. The engine underneath zeo words several of them
# differently, so `regexp::cruby_regex_error` maps them; `a{2,1}` it accepts
# outright, so that one is checked before the build.
["[invalid", "(a", "a)", "a{2,1}", "[z-a]", "*a", "(?"].each do |src|
  begin
    Regexp.new(src)
    p [src, :ok]
  rescue RegexpError => e
    p [src, e.message]
  end
end

# Matching READS characters, so a subject whose bytes are not valid in its
# own encoding is refused rather than matched against replacement characters.
s = "\xff".force_encoding("UTF-8")
p s.valid_encoding?
[
  -> { s =~ /a/ }, -> { s.match(/a/) }, -> { s.match?(/a/) },
  -> { s.scan(/a/) }, -> { s.gsub(/a/, "b") }, -> { s.split(/a/) },
  -> { /a/ =~ s }, -> { /a/.match(s) }, -> { s.index(/a/) },
].each do |probe|
  begin
    probe.call
    p :no_raise
  rescue => e
    p [e.class, e.message]
  end
end
# A BINARY string has no invalid bytes, so it matches.
p(("\xff".force_encoding("ASCII-8BIT") =~ /a/))
# ...and a plain substring search moves by bytes, so it takes no guard.
p s.index("a")
__END__
["[invalid", "premature end of char-class: /[invalid/"]
["(a", "end pattern with unmatched parenthesis: /(a/"]
["a)", "unmatched close parenthesis: /a)/"]
["a{2,1}", "upper is smaller than lower in repeat range: /a{2,1}/"]
["[z-a]", "empty range in char class: /[z-a]/"]
["*a", "target of repeat operator is not specified: /*a/"]
["(?", "end pattern in group: /(?/"]
false
[ArgumentError, "invalid byte sequence in UTF-8"]
[ArgumentError, "invalid byte sequence in UTF-8"]
[ArgumentError, "invalid byte sequence in UTF-8"]
[ArgumentError, "invalid byte sequence in UTF-8"]
[ArgumentError, "invalid byte sequence in UTF-8"]
[ArgumentError, "invalid byte sequence in UTF-8"]
[ArgumentError, "invalid byte sequence in UTF-8"]
[ArgumentError, "invalid byte sequence in UTF-8"]
[ArgumentError, "invalid byte sequence in UTF-8"]
nil
nil

# Regexp reflection/class-method surface
# #2492 casefold?, #2493 option constants, #2494 last_match, #2495 try_convert,
# #2496 options, #2497 ~, #2498/#2527 timeout, #2508/#2509 union, #2528 new(re)
p(/abc/i.casefold?)
p(/abc/.casefold?)
p(Regexp::IGNORECASE)
p(Regexp::EXTENDED)
p(Regexp::MULTILINE)
p(/abc/.options)
p(/abc/i.options)
p(/abc/m.options)
"abc" =~ /b/
p(Regexp.last_match)
p(Regexp.last_match(0))
p(Regexp.try_convert(/x/))
p(Regexp.try_convert("x"))
p(Regexp.new("ab").options)
p(/re/.timeout)
p(Regexp.timeout)
r = Regexp.new(/ab/i)
p(r.source)
p(r.casefold?)
arr = [/foo/, /bar/]
p(Regexp.union(arr))
p(Regexp.union(/foo/i, /bar/).match?("FOO"))
$_ = "hello world"
p(~/world/)
p(~/xyz/)

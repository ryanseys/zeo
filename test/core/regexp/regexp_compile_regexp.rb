p(Regexp.compile(/ab/i).source)
p(Regexp.compile(/ab/i).options)
p(Regexp.compile("cd").source)
p(Regexp.new(/ab/i).source)
p(Regexp.compile(/ab/i) =~ "AB")
__END__
"ab"
1
"cd"
"ab"
0

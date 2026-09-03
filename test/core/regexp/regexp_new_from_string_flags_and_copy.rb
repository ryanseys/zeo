p(Regexp.new("a.c") =~ "xabc")
p Regexp.new("hi", Regexp::IGNORECASE).match?("HI")
p Regexp.new(/z/i).match?("Z")
p Regexp.new("a.c").source
__END__
1
true
true
"a.c"

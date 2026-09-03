p("foo".match(/\Gfoo/))
p("abcabc".match(/(?<x>abc)\g<x>/))
p("xfoo".match(/\Gfoo/))
p("abcabc".match(/(?<x>abc)\g<x>/)[1])
p("aaa".scan(/\Ga/).length)
p("abcabcabc".match(/(abc)\g<1>\g<1>/)[0])
__END__
#<MatchData "foo">
#<MatchData "abcabc" x:"abc">
nil
"abc"
3
"abcabcabc"

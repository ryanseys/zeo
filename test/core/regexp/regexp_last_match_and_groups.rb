"abc123" =~ /([a-z]+)(\d+)/
p Regexp.last_match(1)
p Regexp.last_match(2)
__END__
"abc"
"123"

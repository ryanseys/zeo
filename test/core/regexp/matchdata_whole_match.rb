# $~[0] is the whole match even when capture groups are present; $~[n] are groups.
"hello world" =~ /(\w+) (\w+)/
p $~[0]
p $~[1]
p $~[2]
__END__
"hello world"
"hello"
"world"

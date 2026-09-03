puts(/abc/ =~ "xxabcxx")
r = /foo/
puts r.source
puts(r === "foobar")
puts(r === "baz")
__END__
2
foo
true
false

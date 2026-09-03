puts("hello" !~ /xyz/)
puts("hello" !~ /l+/)
puts(/xyz/ !~ "hello")
__END__
true
false
true

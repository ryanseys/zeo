require "base64"

puts Base64.encode64("hello world")
puts Base64.encode64("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
puts Base64.strict_encode64("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
puts Base64.decode64(Base64.encode64("round trip"))
puts Base64.urlsafe_encode64("ab")
puts Base64.urlsafe_decode64("YWI")
puts Base64.strict_decode64("aGVsbG8=")
__END__
aGVsbG8gd29ybGQ=
YWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFh
YWFhYWE=
YWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWE=
round trip
YWI=
ab
hello

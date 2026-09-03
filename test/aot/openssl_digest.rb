# The vendored OpenSSL links: digests and HMAC come from it, not from the
# system library.
require "openssl"
puts OpenSSL::Digest::SHA256.hexdigest("zeo")
puts OpenSSL::HMAC.hexdigest("SHA256", "key", "message")
puts OpenSSL::Digest.new("SHA1").hexdigest("abc")
__END__
3d6780af3250f8de07aa3b0b6daccc970b0692d73e566d957e8152f570e3609a
6e9ef29b75fffc5b7abae527d58fdadb2fe42e7219011976917343065f58ed4a
a9993e364706816aba3e25717850c26c9cd0d89d

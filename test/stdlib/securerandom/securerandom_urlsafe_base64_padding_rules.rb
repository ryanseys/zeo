require "securerandom"
# Default: no padding, URL/filename-safe alphabet only. n=5 isn't a
# multiple of 3, so a padded encoding really does carry a trailing '='.
s = SecureRandom.urlsafe_base64(5)
puts(s =~ /\A[A-Za-z0-9\-_]+\z/ ? "urlsafe_ok" : "urlsafe_BAD: #{s}")
puts s.include?("=")
# Explicit padding: keeps the '='.
puts SecureRandom.urlsafe_base64(5, true).include?("=")
__END__
urlsafe_ok
false
true

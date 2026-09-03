require "openssl"

# Constant-time string comparison, length-independent: true iff equal.
p OpenSSL.secure_compare("s3cr3t-token", "s3cr3t-token")
p OpenSSL.secure_compare("s3cr3t-token", "s3cr3t-tokeX")
p OpenSSL.secure_compare("short", "much longer value")

# fixed_length_secure_compare demands equal-length inputs.
p OpenSSL.fixed_length_secure_compare("0123456789abcdef", "0123456789abcdef")
p OpenSSL.fixed_length_secure_compare("0123456789abcdef", "0123456789abcdeX")
begin
  OpenSSL.fixed_length_secure_compare("abc", "abcd")
rescue ArgumentError => e
  puts "ArgumentError: #{e.message}"
end
__END__
true
false
false
true
false
ArgumentError: inputs must be of equal length

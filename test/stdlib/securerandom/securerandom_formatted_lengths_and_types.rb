require "securerandom"
puts SecureRandom.hex(8).length          # 2*n
puts SecureRandom.hex.length             # default n=16
puts SecureRandom.hex(8).class
puts SecureRandom.base64(6).length        # 4/3 * n, padded
puts SecureRandom.random_bytes(5).bytesize
puts SecureRandom.random_bytes(5).encoding.to_s
puts SecureRandom.alphanumeric(12).length
puts(SecureRandom.alphanumeric(20) =~ /\A[A-Za-z0-9]{20}\z/ ? "alnum_ok" : "alnum_BAD")
__END__
16
32
String
8
5
ASCII-8BIT
12
alnum_ok

require "digest"
require "digest/bubblebabble"

puts Digest::MD5.hexdigest("abc"), Digest::SHA1.hexdigest("abc"), Digest::SHA256.hexdigest("abc")
puts Digest::SHA384.hexdigest("abc"), Digest::SHA512.base64digest("abc"), Digest::RMD160.hexdigest("abc")
d = Digest::SHA256.new
d << "a"
d.update("bc")
puts d.hexdigest, d.digest_length, d.block_length, d == Digest::SHA256.hexdigest("abc")
puts Digest::SHA1.bubblebabble("abc"), Digest("MD5").name, Digest::MD5.file(__FILE__).hexdigest.size

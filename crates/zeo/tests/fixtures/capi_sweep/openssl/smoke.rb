require "openssl"

puts OpenSSL::Digest::SHA256.hexdigest("abc"), OpenSSL::Digest.new("SHA1").update("abc").hexdigest
puts OpenSSL::HMAC.hexdigest("SHA256", "key", "data")
c = OpenSSL::Cipher.new("aes-128-cbc")
c.encrypt
c.key = "k" * 16
c.iv = "i" * 16
ct = c.update("hello world") + c.final
puts ct.unpack1("H*")
d = OpenSSL::Cipher.new("aes-128-cbc")
d.decrypt
d.key = "k" * 16
d.iv = "i" * 16
puts d.update(ct) + d.final
puts OpenSSL::BN.new("12345678901234567890") * 2, OpenSSL::BN.new(255).to_s(16)
puts OpenSSL::PKCS5.pbkdf2_hmac("pw", "salt", 1000, 16, "sha256").unpack1("H*")
key = OpenSSL::PKey::RSA.new(2048)
puts key.verify("SHA256", key.sign("SHA256", "msg"), "msg"), key.public_key.class
puts OpenSSL::SSL::SSLContext.new.class, OpenSSL::X509::Name.parse("/CN=zeo").to_s
puts OpenSSL::Random.random_bytes(8).bytesize

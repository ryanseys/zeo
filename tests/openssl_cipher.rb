# OpenSSL::Cipher over the vendored libcrypto EVP. Legacy-provider names
# (RC4, BF-CBC) fail at new under OpenSSL 3, in zeo as under CRuby.
require "openssl"

puts "-- metadata --"
c = OpenSSL::Cipher.new("AES-256-CBC")
p c.class
p c.name
p c.key_len
p c.iv_len
p c.block_size
p c.authenticated?
p OpenSSL::Cipher.new("aes-256-cbc").name
p OpenSSL::Cipher.new("chacha20-poly1305").name
p OpenSSL::Cipher.new("aes-256-gcm").authenticated?
p OpenSSL::Cipher.ciphers.size > 20
p OpenSSL::Cipher.ciphers.include?("aes-256-cbc")
p OpenSSL::Cipher.ciphers.include?("AES-256-CBC")

puts "-- cbc round trip --"
key = "\x01".b * 32
iv  = "\x02".b * 16
c.encrypt
c.key = key
c.iv = iv
ct = c.update("secret message!!") + c.final
p ct.unpack1("H*")
p ct.encoding
d = OpenSSL::Cipher.new("AES-256-CBC")
d.decrypt
d.key = key
d.iv = iv
p d.update(ct) + d.final

puts "-- ctr --"
c3 = OpenSSL::Cipher.new("AES-128-CTR")
c3.encrypt
c3.key = "\x03".b * 16
c3.iv = "\x04".b * 16
p (c3.update("stream me") + c3.final).unpack1("H*")

puts "-- gcm --"
g = OpenSSL::Cipher.new("aes-256-gcm")
g.encrypt
g.key = key
g.iv_len = 12
g.iv = "\x05".b * 12
g.auth_data = "aad"
gct = g.update("authenticated!") + g.final
tag = g.auth_tag
p gct.unpack1("H*")
p tag.bytesize
p tag.unpack1("H*")
gd = OpenSSL::Cipher.new("aes-256-gcm")
gd.decrypt
gd.key = key
gd.iv = "\x05".b * 12
gd.auth_tag = tag
gd.auth_data = "aad"
p gd.update(gct) + gd.final
bd = OpenSSL::Cipher.new("aes-256-gcm")
bd.decrypt
bd.key = key
bd.iv = "\x05".b * 12
bd.auth_tag = tag.succ
bd.auth_data = "aad"
begin
  bd.update(gct) + bd.final
rescue => e
  p [e.class, e.message]
end

puts "-- chacha20-poly1305 --"
ch = OpenSSL::Cipher.new("chacha20-poly1305")
ch.encrypt
ch.key = key
ch.iv = "\x06".b * 12
ch.auth_data = ""
cct = ch.update("chacha!") + ch.final
p cct.unpack1("H*")
p ch.auth_tag.unpack1("H*")

puts "-- padding + errors --"
np = OpenSSL::Cipher.new("AES-256-CBC")
np.encrypt
np.key = key
np.iv = iv
np.padding = 0
p (np.update("0123456789abcdef") + np.final).unpack1("H*")
begin
  np2 = OpenSSL::Cipher.new("AES-256-CBC")
  np2.encrypt
  np2.key = key
  np2.iv = iv
  np2.padding = 0
  np2.update("short") + np2.final
rescue => e
  p [e.class, e.message]
end
begin
  OpenSSL::Cipher.new("NOPE-CIPHER")
rescue => e
  p e.class
end
begin
  x = OpenSSL::Cipher.new("AES-256-CBC")
  x.encrypt
  x.key = "\x01".b * 16
rescue => e
  p [e.class, e.message]
end
begin
  y = OpenSSL::Cipher.new("AES-256-CBC")
  y.decrypt
  y.key = key
  y.iv = iv
  y.update("x" * 16) + y.final
rescue => e
  p [e.class, e.message]
end
begin
  z = OpenSSL::Cipher.new("AES-256-CBC")
  z.update("no key set")
rescue => e
  p [e.class, e.message]
end
# RC4/BF live in OpenSSL 3's legacy provider, so `new` fails on both
# runtimes. (Camellia/IDEA/SEED are additionally absent from zeo's vendored
# build -- a documented divergence, so they are not exercised here.)
%w[DES-EDE3-CBC RC4 BF-CBC aes-192-cbc].each do |n|
  begin
    OpenSSL::Cipher.new(n)
    puts "#{n}: ok"
  rescue OpenSSL::Cipher::CipherError
    puts "#{n}: CipherError"
  end
end

puts "-- reset + random + keyivgen --"
r = OpenSSL::Cipher.new("AES-256-CBC")
r.encrypt
r.key = key
r.iv = iv
a1 = r.update("hello") + r.final
r.reset
r.iv = iv
a2 = r.update("hello") + r.final
p a1 == a2
rk = OpenSSL::Cipher.new("aes-128-ctr")
rk.encrypt
p rk.random_key.bytesize
p rk.random_iv.bytesize
kv = OpenSSL::Cipher.new("aes-256-cbc")
kv.encrypt
kv.pkcs5_keyivgen("passphrase")
e1 = kv.update("hello") + kv.final
kv2 = OpenSSL::Cipher.new("aes-256-cbc")
kv2.decrypt
kv2.pkcs5_keyivgen("passphrase")
p kv2.update(e1) + kv2.final
p e1.unpack1("H*")

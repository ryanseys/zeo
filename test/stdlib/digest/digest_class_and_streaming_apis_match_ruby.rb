require "digest"
puts Digest::SHA256.hexdigest("abc")
puts Digest::MD5.hexdigest("")
d = Digest::SHA256.new
d << "a"; d.update("bc")
puts d.hexdigest
puts Digest::SHA256.base64digest("abc")
__END__
ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
d41d8cd98f00b204e9800998ecf8427e
ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=

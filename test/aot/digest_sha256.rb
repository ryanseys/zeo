# The digest extension links: the hash is computed by native code in the
# binary.
require "digest"
puts Digest::SHA256.hexdigest("zeo")
puts Digest::MD5.hexdigest("")
d = Digest::SHA1.new
d << "a"
d << "b"
puts d.hexdigest
__END__
3d6780af3250f8de07aa3b0b6daccc970b0692d73e566d957e8152f570e3609a
d41d8cd98f00b204e9800998ecf8427e
da23614e02469a0d7c7bd1bdab5c9c474b1904dc

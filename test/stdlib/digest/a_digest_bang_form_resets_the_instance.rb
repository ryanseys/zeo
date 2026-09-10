# `Digest::Instance`'s three bang forms answer the current digest and then put
# the instance back to empty; the plain forms leave it alone. rubygems reaches
# `hexdigest!` for every file it unpacks, so `zeo gem install` needed them.

require "tmpdir"
ZTMP = Dir.mktmpdir

require "digest"

d = Digest::SHA256.new
d << "abc"
p d.hexdigest!
p d.hexdigest

d << "abc"
p d.digest!.bytesize
p d.digest.bytesize

d << "abc"
p d.base64digest!
p d.base64digest

# The instance half of `file`, which answers self so a chain reads left to right.
File.write(File.join(ZTMP, "digest_bang_probe.txt"), "abc")
p Digest::SHA256.new.file(File.join(ZTMP, "digest_bang_probe.txt")).hexdigest
File.unlink(File.join(ZTMP, "digest_bang_probe.txt"))

p Digest::SHA256.new.respond_to?(:hexdigest!)
__END__
"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
32
32
"ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0="
"47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU="
"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
true

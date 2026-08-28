# `Digest::Instance`'s three bang forms answer the current digest and then put
# the instance back to empty; the plain forms leave it alone. rubygems reaches
# `hexdigest!` for every file it unpacks, so `zeo gem install` needed them.

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
File.write("digest_bang_probe.txt", "abc")
p Digest::SHA256.new.file("digest_bang_probe.txt").hexdigest
File.unlink("digest_bang_probe.txt")

p Digest::SHA256.new.respond_to?(:hexdigest!)

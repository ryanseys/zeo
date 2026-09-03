require "digest"
def show(l); print l, ": "; p(yield); rescue Exception => e; puts "#{e.class}: #{e.message}"; end
show("ancestors") { Digest::MD5.ancestors.first(5) }
show("sha2 superclass") { Digest::SHA2.superclass }
show("sha384 superclass") { Digest::SHA384.superclass }
show("version") { Digest::VERSION }
show("class file") do
  d = Digest::SHA256.file(__FILE__)
  [d.class, d.hexdigest == Digest::SHA256.hexdigest(File.binread(__FILE__))]
end
show("base64digest") { Digest::SHA256.base64digest("abc") }
show("inst base64digest") { Digest::SHA1.new.base64digest("abc") }
show("bang") { d = Digest::SHA256.new; d << "ab"; [d.base64digest!, d.hexdigest] }
show("file owner") { Digest::MD5.instance_method(:file).owner }
show("Digest fn") { Digest(:SHA256).equal?(Digest::SHA256) }
show("Digest nope") { Digest(:NOPE) }
show("require nope") { require "digest/nope" }
show("require sha2") { require "digest/sha2" }
show("hexencode") { Digest.hexencode("ab") }
show("instance module") { Digest::Instance.instance_of?(Module) }
show("class includes") { Digest::Class.include?(Digest::Instance) }
__END__
ancestors: [Digest::MD5, Digest::Base, Digest::Class, Digest::Instance, Object]
sha2 superclass: Digest::Class
sha384 superclass: Digest::Base
version: "3.2.1"
class file: [Digest::SHA256, true]
base64digest: "ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0="
inst base64digest: "qZk+NkcGgWq6PiVxeFDCbJzQ2J0="
bang: ["+44g/C5MPySMYMOb1lLzwTRymLuXe4tNWQO4UFViBgM=", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"]
file owner: Digest::Instance
Digest fn: true
Digest nope: LoadError: library not found for class Digest::NOPE -- digest/nope
require nope: LoadError: cannot load such file -- digest/nope
require sha2: false
hexencode: "6162"
instance module: true
class includes: true

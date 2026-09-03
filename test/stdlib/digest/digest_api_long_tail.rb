# Digest rows beyond the (matching) core: `Digest::SHA2` (the
# bit-length-parameterized class) is absent, `Digest::SHA256.file` is
# absent, and #inspect embeds the current hexdigest. (Found by the
# 2026-08-24 probe sweep.)
require "digest"
require "tempfile"
def show
  p yield
rescue Exception => e
  puts "#{e.class}: #{e.message}"
end
show { Digest::SHA2.new(384).hexdigest("x").length }
show { Digest::SHA2.new(224) }
show { Tempfile.create("dg") { |f| f.write("filedata"); f.flush; Digest::SHA256.file(f.path).hexdigest == Digest::SHA256.hexdigest("filedata") } }
show { d = Digest::MD5.new; d << "a"; d.inspect.include?(d.hexdigest) }
__END__
96
ArgumentError: unsupported bit length: 224
true
true

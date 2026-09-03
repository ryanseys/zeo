# The ext tail of the sweep: two threads Psych-load the same file
# while a third draws OS entropy, all in ZEO_GVL=1 mode.
#@ env: ZEO_GVL=1

require "yaml"
require "openssl"
y = File.join(ENV["TMPDIR"] || "/tmp", "zeo_yaml_probe_#{Process.pid}.yml")
File.write(y, "name: zeo\ncount: 3\n")
loads = 2.times.map { Thread.new { YAML.load_file(y) } }
entropy = Thread.new { OpenSSL::Random.random_bytes(16) }
docs = loads.map(&:value)
p docs[0]["name"]
p docs[1]["count"]
p entropy.value.bytesize
File.delete(y)
__END__
"zeo"
3
16

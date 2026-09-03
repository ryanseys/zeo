# In-tree ext/ extensions, activated by `require` and dispatched through the
# same tables as the core builtins. Each is behind a per-extension cargo
# feature (default on). See docs/EXTENSIONS.md.

require "json"
require "yaml"
require "stringio"
require "strscan"
require "cgi/escape"
require "digest"
require "zlib"

# --- json (serde_json parse + hand-rolled generate) ---
puts JSON.generate({ "a" => 1, "b" => [2, 3.5, nil, true] })
puts JSON.parse('{"x":[1,2,3],"y":"hi"}').inspect
puts JSON.pretty_generate({ "k" => [1, 2] })

# --- yaml / psych (yaml-rust2 load + Psych-style dump) ---
print YAML.dump({ "name" => "zeo", "tags" => ["aot", "ruby"] })
puts YAML.load("port: 8080\nhosts:\n- a\n- b").inspect

# --- stringio (in-memory IO buffer) ---
io = StringIO.new
io.puts "line one"
io.puts "line two"
io.rewind
io.each_line { |l| print "got: #{l}" }

# --- strscan (lexer over the Regexp engine) ---
sc = StringScanner.new("key = value")
puts sc.scan(/\w+/)
sc.skip(/\s*=\s*/)
puts sc.scan(/\w+/)

# --- cgi (url/html escaping) ---
puts CGI.escape("name=zeo rs&v=1")
puts CGI.escapeHTML("<b>bold & 'quoted'</b>")

# --- digest (RustCrypto hashing) ---
puts Digest::SHA256.hexdigest("hello")
puts Digest::MD5.hexdigest("hello")

# --- zlib (checksums) ---
puts Zlib.crc32("hello")
puts Zlib.adler32("hello")
__END__
{"a":1,"b":[2,3.5,null,true]}
{"x" => [1, 2, 3], "y" => "hi"}
{
  "k": [
    1,
    2
  ]
}
---
name: zeo
tags:
- aot
- ruby
{"port" => 8080, "hosts" => ["a", "b"]}
got: line one
got: line two
key
value
name%3Dzeo+rs%26v%3D1
&lt;b&gt;bold &amp; &#39;quoted&#39;&lt;/b&gt;
2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
5d41402abc4b2a76b9719d911017c592
907060870
103547413

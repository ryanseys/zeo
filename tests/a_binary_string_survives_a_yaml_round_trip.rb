# A BINARY string is not valid UTF-8, so YAML carries it as a `!!binary`
# base64 scalar and loads it back byte-for-byte. zeo's round trip answers a
# string that no longer equals the one it dumped.
#
# Carved out of tests/probe_roundtrip.rb ("yaml:string_binary").
require "yaml"

s = "\xff\xfe".b
back = YAML.unsafe_load(YAML.dump(s))
puts back == s
puts back.encoding
puts back.bytes.inspect

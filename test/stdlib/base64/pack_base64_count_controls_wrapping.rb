# `m0` is one unbroken run with no trailing newline (RFC 4648); a bare `m`
# wraps at 45 input bytes / 60 columns with a trailing newline; empty
# input yields "" for both.

p ["hello world"].pack("m0")
p [""].pack("m0")
p ["a\x00b".dup].pack("m0")
p ["hi"].pack("m")
p [""].pack("m")
p [("A" * 50)].pack("m")
__END__
"aGVsbG8gd29ybGQ="
""
"YQBi"
"aGk=\n"
""
"QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFB\nQUFBQUE=\n"

require "stringio"

s = StringIO.new("hello")
p s.read(2)
p s.getc
p s.ungetc("Z")
p s.read
__END__
"he"
"l"
nil
"Zlo"

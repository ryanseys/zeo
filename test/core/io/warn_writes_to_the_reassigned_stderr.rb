# One argument and several, collected in a StringIO bound to $stderr.
# (spinel issue #3113)
require "stringio"
out = StringIO.new
$stderr = out
warn "x"
warn "y", "z"
raise if out.string.empty?
p out.string
__END__
"x\ny\nz\n"

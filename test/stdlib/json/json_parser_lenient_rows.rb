# Three parser rows where ruby's json is LOOSER or differently typed
# than zeo's serde path: `-0` parses as INTEGER 0 (zeo: Float -0.0),
# `allow_trailing_comma: true` admits `[1,]`, and trailing `// c` after
# the document is accepted.
require "json"
def show
  p yield
rescue Exception => e
  puts "#{e.class}: #{e.message}"
end
show { JSON.parse("[-0]").first }
show { JSON.parse("[1,]", allow_trailing_comma: true) }
show { JSON.parse("[1] // c") }
__END__
0
[1]
[1]

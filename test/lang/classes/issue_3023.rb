e = LocalJumpError.new("no block given")
p e.class
p e.message
p e.is_a?(StandardError)
p e.is_a?(Exception)
s = ScriptError.new("y")
p s.class
p s.is_a?(Exception)
p s.is_a?(StandardError)
x = SystemExit.new
p x.is_a?(Exception)
p x.is_a?(StandardError)
__END__
LocalJumpError
"no block given"
true
true
ScriptError
true
false
true
false

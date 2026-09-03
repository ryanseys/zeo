class SubX < StandardError; end
p(SubX.new("m").cause)
p(SubX.new("m").backtrace)
p(SubX.new("m").message)
p(SubX.new("m").detailed_message)
p(SubX.new("m").exception.class)
e = (begin; raise SubX, "z"; rescue SubX => x; x; end)
p e.cause
p e.message
__END__
nil
nil
"m"
"m (SubX)"
SubX
nil
"z"

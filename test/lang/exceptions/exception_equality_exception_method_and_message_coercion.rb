e = RuntimeError.new("m")
e2 = RuntimeError.new("m")
p e == e2
p e == RuntimeError.new("other")
p e == "not an exception"
p e.eql?(e)
p e.eql?(e2)
p e.exception.equal?(e)
p e.exception("n").message
p e.exception("n").equal?(e)
p RuntimeError.exception("z").message
p RuntimeError.exception("z").class
p RuntimeError.new(42).message
p RuntimeError.new(:sym).message
p ArgumentError.new([1, 2]).message
p [true, false].include?(Exception.to_tty?)
__END__
true
false
false
true
false
true
"n"
false
"z"
RuntimeError
"42"
"sym"
"[1, 2]"
true

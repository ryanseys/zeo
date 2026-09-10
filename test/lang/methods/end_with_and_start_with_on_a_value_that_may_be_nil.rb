# A method answering a String or nil, passed to one that calls end_with? or
# start_with? on it.
def maybe_string(flag) = flag ? "foo" : nil
def crash(value) = value.end_with?("o")
p crash(maybe_string(true))
def sw(v) = v.start_with?("f")
p sw(maybe_string(true))
__END__
true
true

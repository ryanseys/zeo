def maybe_string(flag) = flag ? "foo" : nil
def crash(value) = value.end_with?("o")
p crash(maybe_string(true))
def sw(v) = v.start_with?("f")
p sw(maybe_string(true))

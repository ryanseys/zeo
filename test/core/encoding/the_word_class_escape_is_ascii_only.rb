# `\w` matches [a-zA-Z0-9_] and stops at an accented letter.
p "café bar".scan(/\w+/)
__END__
["caf", "bar"]

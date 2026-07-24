# String#each_line yields each line WITH its trailing newline
# (CRuby behaviour). Last line keeps no newline only when the
# source itself didn't end with one.
"hello\nworld".each_line { |line| puts line.inspect }

# Source with trailing newline — last yielded line includes it.
"a\nb\n".each_line { |line| puts line.inspect }

# Single line, no newline.
"only".each_line { |line| puts line.inspect }

# Empty input — block never fires.
out = []
"".each_line { |line| out.push(line) }
puts out.length

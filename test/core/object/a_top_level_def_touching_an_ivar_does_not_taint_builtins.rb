# `mro::materialize_methods` collects ivars from every ancestor, but a
# BUILTIN never materializes Object's methods -- so the ivar loop has to
# skip Object for builtins exactly as the method loop does. It didn't,
# which made this program fail to compile with a rejection blaming
# `Integer` for a `@count` that Integer has nothing to do with.

def bump; @count = (@count || 0) + 1; end
bump
p @count
p 1 + 2
p "s".length
p [1, 2].map { |i| i * 2 }
__END__
1
3
1
[2, 4]

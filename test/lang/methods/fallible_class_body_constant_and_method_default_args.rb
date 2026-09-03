# `CONST = <fallible expr>` in a class body and `def m(a = <fallible>)`
# both previously emitted `?` outside a Result context.

def source
  41
end
class Config
  LIMIT = [1, 2, 3].sum
end
def bump(a = source + 1)
  a
end
puts Config::LIMIT
puts bump
puts bump(5)
__END__
6
42
5

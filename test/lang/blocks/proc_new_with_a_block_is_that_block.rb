p Proc.new { |x| x * 2 }.call(4)
p Proc.new { |a, b| [a, b] }.arity
p Proc.new {}.lambda?
__END__
8
2
false

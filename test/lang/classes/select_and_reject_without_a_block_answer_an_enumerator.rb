# The enumerator's class and next, the block forms beside them, and
# `select.with_index`.
x = (1..3).select
p x.class
p x.next
p [1, 2, 3].select.class
p [1, 2, 3].reject.class
p((1..3).select { |i| i.odd? })
p((1..3).reject { |i| i.odd? })
p [1, 2, 3].select.with_index { |v, i| i > 0 }
__END__
Enumerator
1
Enumerator
Enumerator
[1, 3]
[2]
[2, 3]

x = (1..3).select
p x.class
p x.next
p [1, 2, 3].select.class
p [1, 2, 3].reject.class
p((1..3).select { |i| i.odd? })
p((1..3).reject { |i| i.odd? })
p [1, 2, 3].select.with_index { |v, i| i > 0 }

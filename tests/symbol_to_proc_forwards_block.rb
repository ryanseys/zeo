p :map.to_proc.call([1, 2]) { |x| x * 3 }
p :each.to_proc.call([1, 2]) { |x| }
p :select.to_proc.call([1, 2, 3]) { |x| x.odd? }

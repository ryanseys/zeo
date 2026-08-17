v = (1..5).each_cons(2) { |s| s }
p v

v = (1..5).each_slice(2) { |s| s }; p v        # Ruby: 1..5    Spinel: [1, 2, 3, 4, 5]
v = (1..5).each_cons(2) { |s| s }; p v.class   # Ruby: Range   Spinel: Array

p((1..5).each_cons(2) { |s| s })                  # => 1..5
p((1..5).each_slice(2) { |s| s })                 # => 1..5
v = [1, 2, 3].each_cons(2) { |s| s }; p v         # => [1, 2, 3]
v = [1, 2, 3].each_slice(2) { |s| s }; p v        # => [1, 2, 3]
v = (1..5).each { |s| s }; p v                    # => 1..5
v = (1..5).each_entry { |s| s }; p v              # => 1..5

v = (1..5).reverse_each { |s| s }; p v
h = { a: 1, b: 2 }
v = h.each_slice(1) { |s| s }; p v
p((1..6).each_cons(3) { |s| s })
acc = []; (1..5).each_slice(2) { |s| acc << s }; p acc

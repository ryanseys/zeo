# Block and Proc argument semantics.

# Hash#each yields each pair as ONE array, so a block sees it whole or splits it.
h = { a: 1, b: 2 }
h.each { |pair| p pair }             # [:a, 1] / [:b, 2]
h.each { |k, v| puts "#{k}=#{v}" }   # a=1 / b=2

# A forwarded 1-arg callable receives the pair array (no arity error).
def show(pair) = p pair
h.each(&method(:show))               # [:a, 1] / [:b, 2]

# A proc's keyword params bind from the call's kwargs; a required keyword
# omitted raises, an optional one defaults.
add = proc { |a:, b:| a + b }
p add.call(a: 1, b: 2)               # 3
opt = proc { |x:, y: 100| [x, y] }
p opt.call(x: 5)                     # [5, 100]
begin
  add.call(a: 1)
rescue ArgumentError => e
  puts e.message                     # missing keyword: :b
end

# A lambda with keyword-only params counts the kwargs hash as keywords, not
# a positional argument.
lam = ->(x:, y: 9) { [x, y] }
p lam.call(x: 5)                     # [5, 9]

# Non-lambda blocks are lenient: extra args drop, missing ones nil-fill, and
# a single array auto-splats across multiple params.
pr = proc { |a, b| [a, b] }
p pr.call(1, 2, 3)                   # [1, 2]
p pr.call(1)                         # [1, nil]
p [[1, 2], [3, 4]].map { |a, b| a + b }   # [3, 7]
__END__
[:a, 1]
[:b, 2]
a=1
b=2
[:a, 1]
[:b, 2]
3
[5, 100]
missing keyword: :b
[5, 9]
[1, 2]
[1, nil]
[3, 7]

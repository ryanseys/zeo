# The full String/Array/Hash mutator matrix against a frozen receiver
# -- every row's outcome (FrozenError, or legal like `str * 2`) is
# verbatim ruby 4.0.6. Guard-ordering nuances included: `setbyte`
# validates index/type BEFORE the frozen check; `transform_values!`
# returns its blockless enumerator before it.

def try(label)
  yield
  puts "#{label}: ok"
rescue => e
  puts "#{label}: #{e.class}: #{e.message}"
end

s = "abc".freeze
try("str <<") { s << "d" }
try("str concat") { s.concat("d") }
try("str insert") { s.insert(0, "x") }
try("str prepend") { s.prepend("x") }
try("str replace") { s.replace("x") }
try("str clear") { s.clear }
try("str chomp!") { s.chomp! }
try("str chop!") { s.chop! }
try("str squeeze!") { s.squeeze! }
try("str strip!") { s.strip! }
try("str lstrip!") { s.lstrip! }
try("str rstrip!") { s.rstrip! }
try("str sub!") { s.sub!(/a/, "z") }
try("str gsub!") { s.gsub!(/a/, "z") }
try("str tr!") { s.tr!("a", "z") }
try("str tr_s!") { s.tr_s!("a", "z") }
try("str delete!") { s.delete!("a") }
try("str upcase!") { s.upcase! }
try("str downcase!") { s.downcase! }
try("str capitalize!") { s.capitalize! }
try("str swapcase!") { s.swapcase! }
try("str succ!") { s.succ! }
try("str reverse!") { s.reverse! }
try("str slice!") { s.slice!(0) }
try("str []=") { s[0] = "z" }
try("str setbyte") { s.setbyte(0, 122) }
try("str *=(noop) freeze-safe read") { s * 2 }

a = [3, 1, 2].freeze
try("arr <<") { a << 4 }
try("arr push") { a.push(4) }
try("arr pop") { a.pop }
try("arr shift") { a.shift }
try("arr unshift") { a.unshift(0) }
try("arr insert") { a.insert(0, 9) }
try("arr []=") { a[0] = 9 }
try("arr delete") { a.delete(1) }
try("arr delete_at") { a.delete_at(0) }
try("arr delete_if") { a.delete_if { true } }
try("arr clear") { a.clear }
try("arr compact!") { a.compact! }
try("arr flatten!") { a.flatten! }
try("arr uniq!") { a.uniq! }
try("arr sort!") { a.sort! }
try("arr sort_by!") { a.sort_by! { |x| x } }
try("arr reverse!") { a.reverse! }
try("arr rotate!") { a.rotate! }
try("arr shuffle!") { a.shuffle! }
try("arr map!") { a.map! { |x| x } }
try("arr select!") { a.select! { true } }
try("arr reject!") { a.reject! { false } }
try("arr keep_if") { a.keep_if { true } }
try("arr fill") { a.fill(0) }
try("arr replace") { a.replace([1]) }
try("arr concat") { a.concat([1]) }

h = { k: 1 }.freeze
try("hash []=") { h[:x] = 2 }
try("hash store") { h.store(:x, 2) }
try("hash delete") { h.delete(:k) }
try("hash delete_if") { h.delete_if { true } }
try("hash clear") { h.clear }
try("hash merge!") { h.merge!({ b: 2 }) }
try("hash update") { h.update({ b: 2 }) }
try("hash reject!") { h.reject! { false } }
try("hash select!") { h.select! { true } }
try("hash keep_if") { h.keep_if { true } }
try("hash compact!") { h.compact! }
try("hash transform_keys!") { h.transform_keys!(&:to_s) }
try("hash transform_values!") { h.transform_values! { |v| v } }
try("hash replace") { h.replace({}) }
try("hash default=") { h.default = 0 }
try("hash shift") { h.shift }
__END__
str <<: FrozenError: can't modify frozen String: "abc"
str concat: FrozenError: can't modify frozen String: "abc"
str insert: FrozenError: can't modify frozen String: "abc"
str prepend: FrozenError: can't modify frozen String: "abc"
str replace: FrozenError: can't modify frozen String: "abc"
str clear: FrozenError: can't modify frozen String: "abc"
str chomp!: FrozenError: can't modify frozen String: "abc"
str chop!: FrozenError: can't modify frozen String: "abc"
str squeeze!: FrozenError: can't modify frozen String: "abc"
str strip!: FrozenError: can't modify frozen String: "abc"
str lstrip!: FrozenError: can't modify frozen String: "abc"
str rstrip!: FrozenError: can't modify frozen String: "abc"
str sub!: FrozenError: can't modify frozen String: "abc"
str gsub!: FrozenError: can't modify frozen String: "abc"
str tr!: FrozenError: can't modify frozen String: "abc"
str tr_s!: FrozenError: can't modify frozen String: "abc"
str delete!: FrozenError: can't modify frozen String: "abc"
str upcase!: FrozenError: can't modify frozen String: "abc"
str downcase!: FrozenError: can't modify frozen String: "abc"
str capitalize!: FrozenError: can't modify frozen String: "abc"
str swapcase!: FrozenError: can't modify frozen String: "abc"
str succ!: FrozenError: can't modify frozen String: "abc"
str reverse!: FrozenError: can't modify frozen String: "abc"
str slice!: FrozenError: can't modify frozen String: "abc"
str []=: FrozenError: can't modify frozen String: "abc"
str setbyte: FrozenError: can't modify frozen String: "abc"
str *=(noop) freeze-safe read: ok
arr <<: FrozenError: can't modify frozen Array: [3, 1, 2]
arr push: FrozenError: can't modify frozen Array: [3, 1, 2]
arr pop: FrozenError: can't modify frozen Array: [3, 1, 2]
arr shift: FrozenError: can't modify frozen Array: [3, 1, 2]
arr unshift: FrozenError: can't modify frozen Array: [3, 1, 2]
arr insert: FrozenError: can't modify frozen Array: [3, 1, 2]
arr []=: FrozenError: can't modify frozen Array: [3, 1, 2]
arr delete: FrozenError: can't modify frozen Array: [3, 1, 2]
arr delete_at: FrozenError: can't modify frozen Array: [3, 1, 2]
arr delete_if: FrozenError: can't modify frozen Array: [3, 1, 2]
arr clear: FrozenError: can't modify frozen Array: [3, 1, 2]
arr compact!: FrozenError: can't modify frozen Array: [3, 1, 2]
arr flatten!: FrozenError: can't modify frozen Array: [3, 1, 2]
arr uniq!: FrozenError: can't modify frozen Array: [3, 1, 2]
arr sort!: FrozenError: can't modify frozen Array: [3, 1, 2]
arr sort_by!: FrozenError: can't modify frozen Array: [3, 1, 2]
arr reverse!: FrozenError: can't modify frozen Array: [3, 1, 2]
arr rotate!: FrozenError: can't modify frozen Array: [3, 1, 2]
arr shuffle!: FrozenError: can't modify frozen Array: [3, 1, 2]
arr map!: FrozenError: can't modify frozen Array: [3, 1, 2]
arr select!: FrozenError: can't modify frozen Array: [3, 1, 2]
arr reject!: FrozenError: can't modify frozen Array: [3, 1, 2]
arr keep_if: FrozenError: can't modify frozen Array: [3, 1, 2]
arr fill: FrozenError: can't modify frozen Array: [3, 1, 2]
arr replace: FrozenError: can't modify frozen Array: [3, 1, 2]
arr concat: FrozenError: can't modify frozen Array: [3, 1, 2]
hash []=: FrozenError: can't modify frozen Hash: {k: 1}
hash store: FrozenError: can't modify frozen Hash: {k: 1}
hash delete: FrozenError: can't modify frozen Hash: {k: 1}
hash delete_if: FrozenError: can't modify frozen Hash: {k: 1}
hash clear: FrozenError: can't modify frozen Hash: {k: 1}
hash merge!: FrozenError: can't modify frozen Hash: {k: 1}
hash update: FrozenError: can't modify frozen Hash: {k: 1}
hash reject!: FrozenError: can't modify frozen Hash: {k: 1}
hash select!: FrozenError: can't modify frozen Hash: {k: 1}
hash keep_if: FrozenError: can't modify frozen Hash: {k: 1}
hash compact!: FrozenError: can't modify frozen Hash: {k: 1}
hash transform_keys!: FrozenError: can't modify frozen Hash: {k: 1}
hash transform_values!: FrozenError: can't modify frozen Hash: {k: 1}
hash replace: FrozenError: can't modify frozen Hash: {k: 1}
hash default=: FrozenError: can't modify frozen Hash: {k: 1}
hash shift: FrozenError: can't modify frozen Hash: {k: 1}

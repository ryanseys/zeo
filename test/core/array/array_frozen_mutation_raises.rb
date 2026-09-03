# Every mutating Array method raises FrozenError on a frozen receiver.

a = [1, 2, 3].freeze
def caught(a, name)
  yield
  "BUG #{name}"
rescue FrozenError => e
  e.message
end
p caught(a, "reverse!") { a.reverse! }
p caught(a, "sort!") { a.sort! }
p caught(a, "delete") { a.delete(1) }
p caught(a, "insert") { a.insert(0, 9) }
p caught(a, "clear") { a.clear }
p caught(a, "pop") { a.pop }
p caught(a, "select!") { a.select! { true } }
__END__
"can't modify frozen Array: [1, 2, 3]"
"can't modify frozen Array: [1, 2, 3]"
"can't modify frozen Array: [1, 2, 3]"
"can't modify frozen Array: [1, 2, 3]"
"can't modify frozen Array: [1, 2, 3]"
"can't modify frozen Array: [1, 2, 3]"
"can't modify frozen Array: [1, 2, 3]"

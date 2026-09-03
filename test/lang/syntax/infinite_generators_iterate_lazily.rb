# An INFINITE generator proves the fiber suspend is real: `next`/`peek`
# advance lazily, and `take`/`first` terminate via the Break-based early
# exit (internal iteration restarts from scratch each time -- CRuby).

inf = Enumerator.new do |y|
  n = 0
  loop do
    y << n
    n += 1
  end
end
p inf.next
p inf.next
p inf.peek
p inf.next
p inf.take(3)
p inf.first(2)
__END__
0
1
2
2
[0, 1, 2]
[0, 1]

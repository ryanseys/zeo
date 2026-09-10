# Four calls whose receiver is only known at run time: a lambda's arity, a
# destructured String's ljust, an Integer's to_s(2), and map! on a row.
ranges = [(1..3)]
p ranges[0].to_a
p [->(x) { x }].map(&:arity)                     # Ruby [1]
[["a", "b"]].each { |k, v| puts "#{k.ljust(3)}." } # Ruby "a  ."
p (0...3).map { |n| n }.each_cons(2).map { |a, b| (a ^ b).to_s(2) }
                                                 # Ruby ["1","11"]
[[1, 2], [3, 4]].last.map! { |x| x * 10 }        # Ruby [30,40]
t = Time.new(2026, 7, 21, 9, 30); p [t][0].hour  # Ruby 9
def r(state); state.merge(x: 1); end
acc = [{}]; acc << r(acc.last)                    # Ruby {x:1}
__END__
[1, 2, 3]
[1]
a  .
["1", "11"]
9

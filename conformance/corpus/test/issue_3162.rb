ranges = [(1..3)]
p ranges[0].to_a
p [->(x) { x }].map(&:arity)                     # Ruby [1]        Spinel: NoMethodError 'arity' (Proc)
[["a", "b"]].each { |k, v| puts "#{k.ljust(3)}." } # Ruby "a  ."     Spinel: NoMethodError 'ljust' (String)
p (0...3).map { |n| n }.each_cons(2).map { |a, b| (a ^ b).to_s(2) }
                                                 # Ruby ["1","11"] Spinel: NoMethodError 'to_s' (Integer)
[[1, 2], [3, 4]].last.map! { |x| x * 10 }        # Ruby [30,40]   Spinel: NoMethodError 'map!' (Array)
t = Time.new(2026, 7, 21, 9, 30); p [t][0].hour  # Ruby 9         Spinel: NoMethodError 'hour'/'year'/... (Time; ALL accessors)
def r(state); state.merge(x: 1); end
acc = [{}]; acc << r(acc.last)                    # Ruby {x:1}     Spinel: NoMethodError 'merge' (Hash; param poisoned by acc.last)

r = ([1, 2].rotate("a") rescue $!.class)
p r

r = ([1, 2].sum("x") rescue $!.class); p r        # Ruby: TypeError      Spinel: C compile abort
r = ([1, 2].product(3) rescue $!.class); p r      # Ruby: TypeError      Spinel: C compile abort
r = ([1, 2].inject(:nope) rescue $!.class); p r   # Ruby: NoMethodError  Spinel: C compile abort

r = ([1, 2].fill(0, "x") rescue $!.class); p r    # => TypeError
p([1, 2].rotate(1))                                # => [2, 1]
p([1, 2].sum(3))                                   # => 6
p([1, 2].product([3]))                             # => [[1, 3], [2, 3]]
p([1, 2].inject(:+))                               # => 3

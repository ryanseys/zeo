# each_cons(n).map { |a, b, ...| } destructures the n-window into element-typed
# params, so arithmetic and comparison behave like the elements rather than the
# window array. A single param |w| still binds the whole window.
p [1, 2, 3, 4].each_cons(2).map { |a, b| a + b }               # [3, 5, 7]
p [1, 2, 3, 4].each_cons(2).map { |a, b| a == 2 ? 99 : a + b }  # [3, 99, 7]
p [1, 2, 3].each_cons(2).map { |a, b| [a, b] }                 # [[1, 2], [2, 3]]
p [1, 2, 3, 4, 5].each_cons(3).map { |a, b, c| a + b + c }     # [6, 9, 12]
p [1, 2, 3, 4].each_cons(2).map { |w| w.sum }                  # [3, 5, 7]
p [1, 2, 3, 4].each_cons(2).map { |(a, b)| a * b }             # [2, 6, 12]

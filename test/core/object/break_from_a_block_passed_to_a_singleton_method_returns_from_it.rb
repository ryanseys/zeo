# `break` in the block yields the method's return value -- the method-body
# lambda's terminal arm folds `Signal::Break` into an `Ok` return, exactly
# as an ordinary method does.

obj = Object.new
def obj.count
  yield 1
  yield 2
  yield 3
  "finished"
end
p(obj.count { |n| break "stop#{n}" if n == 2 })
__END__
"stop2"

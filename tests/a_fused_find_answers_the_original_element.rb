arr = [1, 2, 3, 4]

# The element that surfaces is the one the array held, not whatever the body
# left in the param -- the same rule `select`/`reject` obey.
p arr.find { |x| y = x; x = 99; y > 2 }
p arr.detect { |x| x == 1 }
p arr.find { |x| false }
p [].find { true }

# The block is not called again once one answers.
seen = []
p arr.find { |x| seen << x; x > 2 }
p seen

# `break` overrides the whole call; `next v` supplies the iteration's value.
p(arr.find { |x| break :broke })
p(arr.find { |x| next x > 3 })

# A live view: what the body appends is walked too.
live = [1, 2, 3]
walked = []
p live.find { |x| walked << x; live << 9 if x == 1; x == 9 }
p walked

# An `ifnone` argument is a different method shape and takes the dynamic row.
p arr.find(proc { :nope }) { |x| false }

# A nested escaping block capturing the param keeps a per-iteration binding.
store = []
p arr.find { |x| store << -> { x }; x == 2 }
p store.map(&:call)

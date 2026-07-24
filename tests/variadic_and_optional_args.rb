# Many core methods accept more argument shapes than a single fixed arity:
# an optional count, a variadic list, an alternate block form, or an extra
# position/limit. The same method name binds several signatures, dispatched
# by what actually shows up at the call site.

# --- Array: an optional count changes the RETURN TYPE ----------------------
# Bare `last` answers one element; `last(n)` an array of the last n (clamped
# to what's there). `sample(n)` likewise answers n random DISTINCT elements.
p [1, 2, 3].last          # 3
p [1, 2, 3].last(2)       # [2, 3]
p [1, 2, 3].last(5)       # [1, 2, 3]   -- n past the length takes them all
p [1, 2, 3].sample(3).sort # [1, 2, 3]  -- distinct; sorted so it's stable

# --- Array: a block form as an alternative to a value ----------------------
# `rindex(obj)` searches from the right by ==; `rindex { }` by a predicate.
p [1, 2, 3, 2].rindex(2)             # 3
p [1, 2, 3, 2].rindex { |x| x < 3 }  # 3

# --- Array: `fill` spans, growing the array when asked ----------------------
p([0, 0, 0].fill(9))              # [9, 9, 9]
p([0, 0, 0].fill(9, 1, 1))        # [0, 9, 0]      -- start 1, length 1
p([1, 2, 3].fill(9, 1, 5))        # [1, 9, 9, 9, 9, 9]  -- grows past the end
p([1, 2, 3].fill { |i| i })       # [0, 1, 2]      -- block maps each index

# --- Array: variadic set-ops (the named siblings of `|`/`&`/`-`) -----------
# The operators are binary; the named forms take any number of arrays.
p [1, 2, 3].union([2, 3], [4])                 # [1, 2, 3, 4]
p [1, 2, 3, 4].intersection([2, 3, 4], [3, 4]) # [3, 4]
p [1, 2, 3].difference([2], [4])               # [1, 3]

# --- String: one OR MORE char-set specs, intersected -----------------------
# `count`/`delete` AND every spec together; a leading `^` negates one.
p "hello world".count("lo")         # 5
p "hello world".count("lo", "o")    # 2   -- chars in BOTH "lo" and "o"
p "hello world".count("^l", "lo")   # 2   -- in "lo" but NOT "l"  => the o's

# --- String: an optional start position ------------------------------------
p "hello".rindex("l", 2)   # 2    -- last "l" at or before index 2
p "hello".rindex(/l/)      # 3    -- rindex takes a Regexp too
p "hello".match?(/o/, -1)  # true -- position is end-relative when negative

# --- String: split's optional limit ----------------------------------------
p "a,b,c".split(",", 2)    # ["a", "b,c"]        -- positive caps the fields
p "a,b,,".split(",")       # ["a", "b"]          -- trailing empties dropped
p "a,b,,".split(",", -1)   # ["a", "b", "", ""]  -- negative keeps them
p "one two three".each_line(" ").to_a  # ["one ", "two ", "three"]

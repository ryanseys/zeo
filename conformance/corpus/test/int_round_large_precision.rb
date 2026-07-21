# Integer#round/ceil/floor/truncate(ndigits) must use exact integer
# arithmetic: a double-based implementation loses precision above 2^53
# and casting pow(10,-nd) is undefined once 10^(-nd) > INT64_MAX.
p 1000000000000000005.round(-1)   # 1000000000000000010 (>2^53: float would drop the 5)
p 9007199254740993.round(-3)      # 9007199254741000
p 9007199254740993.floor(-3)      # 9007199254740000
p 9007199254740993.ceil(-3)       # 9007199254741000
p 9007199254740993.truncate(-3)   # 9007199254740000
p(-1000000000000000005.round(-1)) # -1000000000000000010
p 123.round(-20)                  # 0  (10^20 unrepresentable -> 0, no UB)
p 123.ceil(-20)                   # 0

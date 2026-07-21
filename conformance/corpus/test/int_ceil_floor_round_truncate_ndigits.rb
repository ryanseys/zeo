# Integer#ceil/floor/round/truncate with ndigits argument.
# Without ndigits these return self; with negative ndigits they
# round to the nearest 10^(-ndigits). Previously spinel silently
# emitted 0 for any call with an argument.

p 1234.ceil(-2)       # 1300
p 1234.floor(-2)      # 1200
p 1234.round(-2)      # 1200
p 1234.truncate(-2)   # 1200
p -1234.ceil(-2)      # -1200
p -1234.floor(-2)     # -1300
p -1234.truncate(-2)  # -1200
p 1255.ceil(-2)       # 1300
p 1255.floor(-2)      # 1200
p 5.round(-1)         # 10
p 1234.ceil(0)        # 1234 (positive ndigits: no-op)
p 1234.round(2)       # 1234

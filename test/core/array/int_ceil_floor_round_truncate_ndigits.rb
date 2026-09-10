# Integer#ceil, #floor, #round and #truncate with an ndigits argument. With
# no argument each answers self; with a negative ndigits each rounds to the
# nearest power of ten.

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
__END__
1300
1200
1200
1200
-1200
-1300
-1200
1300
1200
10
1234
1234

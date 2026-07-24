# A non-literal ndigits can't pick Integer-vs-Float statically, so the result
# stays Float with the value computed exactly. Regression: it used to ignore the
# argument and truncate to Integer (1.234.round(n)==1, 1234.5.round(-1 var)==1235).
# Positive ndigits matches CRuby in both value and class; for ndigits <= 0 the
# value matches and only #class differs (documented), normalized here via to_i.
n2 = 2
p 1.234.round(n2)        # 1.23 (Float, matches CRuby)
p 3.14159.round(n2)      # 3.14
p 2.71828.ceil(n2)       # 2.72
p 2.71828.floor(n2)      # 2.71
p 9.876.truncate(n2)     # 9.87

nm1 = -1
p 1234.5.round(nm1).to_i # 1230
p 1250.0.floor(nm1).to_i # 1250
p 1241.0.ceil(nm1).to_i  # 1250

n0 = 0
p 1.9.round(n0).to_i     # 2
p 7.8.truncate(n0).to_i  # 7

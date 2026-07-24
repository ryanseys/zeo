# Float#to_s switches to scientific notation when the shortest-decimal point
# position leaves -3..15 -- i.e. |x| >= 1e15 or |x| < 1e-4 -- matching CRuby.
puts 999999999999999.0       # 999999999999999.0  (decpt 15, fixed)
puts 1000000000000000.0      # 1.0e+15            (decpt 16, scientific)
puts 1234567890123456.0      # 1.234567890123456e+15
puts 6402373705728000.0      # 6.402373705728e+15  (18!)
puts 1e16                    # 1.0e+16
puts 0.0001                  # 0.0001             (decpt -3, fixed)
puts 0.00009                 # 9.0e-05            (decpt -4, scientific)
puts 2.220446049250313e-16   # Float::EPSILON

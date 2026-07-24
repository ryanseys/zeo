# Issues #875 #876 #873 #860 #894:
# Integer methods that were not dispatched - magnitude (alias for abs),
# modulo (alias for %), remainder (truncated division remainder, differs
# from % for mixed-sign), size (bytes per mrb_int = 8), gcdlcm.
puts (-42).magnitude
puts 7.modulo(3)
puts (-7).modulo(3)
puts 7.remainder(3)
puts (-7).remainder(3)
puts 12.gcdlcm(18).inspect
puts 1.size
puts 256.size

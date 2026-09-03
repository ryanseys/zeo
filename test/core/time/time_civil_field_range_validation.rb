# Out-of-range civil fields raise ArgumentError; in-range values that
# overflow (Feb 30, the 23:59:60 leap second) roll forward instead.

def c; begin; yield; rescue ArgumentError; "ArgumentError"; end; end
p c { Time.utc(2020, 13, 1) }
p c { Time.utc(2020, 1, 32) }
p c { Time.utc(2020, 1, 1, 25) }
p c { Time.utc(2020, 1, 1, 23, 60) }
p Time.utc(2020, 2, 30).month
p Time.utc(2020, 12, 31, 23, 59, 60).year
__END__
"ArgumentError"
"ArgumentError"
"ArgumentError"
"ArgumentError"
3
2021

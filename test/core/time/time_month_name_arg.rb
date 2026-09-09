# From the spinel corpus (c55d9bdb).
# Time.utc does not accept a month NAME ("feb") in the month position.
#
# The month argument of the civil constructors takes an English month
# abbreviation as well as a number. A plain strtoll read those as zero and the
# constructor rejected them.
p Time.utc(2020, "feb", 4).mon
p Time.utc(2020, "FEB", 4).mon
p Time.utc(2020, "Feb", 4).mon
p Time.utc(2020, "jan", 1).mon
p Time.utc(2020, "dec", 31).mon
p Time.utc(2020, "feb", 4).year
p Time.utc(2020, "feb", 4).day

# only the three-letter form is a name; anything else is read as an Integer
r = (Time.utc(2020, "December", 4).mon rescue $!.class); p r
p Time.utc(2020, "3", 4).mon
p Time.utc(2020, 5, 4).mon
__END__
2
2
2
1
12
2020
4
ArgumentError
3
5

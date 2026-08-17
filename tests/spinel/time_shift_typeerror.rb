# Time shifts by a number of seconds. Anything else is CRuby's TypeError; it
# was read as an integer instead, so a String shifted the clock by zero and a
# nil by whatever the slot held.
t = Time.utc(2020,1,2)
p((t + "x" rescue $!.class))
p((t - "x" rescue $!.class))
p((t + nil rescue $!.class))
p((t + [1] rescue $!.class))
p((t + "x" rescue $!.message))
p((t + 60).min)
p((t - 60).min)
p((t + 1.5).usec)

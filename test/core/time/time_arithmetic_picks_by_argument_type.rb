# `t + n` answers a Time; `t - other_time` answers a Float of seconds; `t -
# n` answers a Time. The argument's type picks.

t = Time.at(1700000000).getutc
p (t + 60).to_i
p (t - 60).to_i
p (Time.at(100) - Time.at(40))
p (Time.at(100) - Time.at(40)).class
p (t + 60).class
__END__
1700000060
1699999940
60.0
Float
Time

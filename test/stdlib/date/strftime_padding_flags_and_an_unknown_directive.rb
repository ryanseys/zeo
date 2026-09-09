# The whole directive table, the `-` and `_` padding flags, and the rule that
# an unknown directive is copied verbatim.

t = Time.at(1700000000).getutc
puts t.strftime("%Y-%m-%d %H:%M:%S")
puts t.strftime("%F %T")
puts t.strftime("%a %A %b %B")
puts t.strftime("%j %u %w %p %I")
puts t.strftime("%z %Z")
puts t.strftime("%y %C %s")
puts t.strftime("100%% literal")
puts t.strftime("%Q")
jan = Time.at(1704067200).getutc
puts jan.strftime("%m|%-m|%_m")
__END__
2023-11-14 22:13:20
2023-11-14 22:13:20
Tue Tuesday Nov November
318 2 2 PM 10
+0000 UTC
23 20 1700000000
100% literal
%Q
01|1| 1

# The whole `strftime` directive table for both engines. `Time` and `date`
# share one renderer, and differ in exactly two places: `date` adds `%Q`/`%+`,
# and a trailing incomplete directive is an ArgumentError for Time where
# `date` echoes it.
t = Time.utc(2024, 2, 29, 5, 7, 9, 123456)
["%c","%x","%X","%D","%F","%T","%R","%r","%v","%s","%L","%3L","%6L","%9L","%N","%3N",
 "%Y %C %y %m %d %e %j %H %k %I %l %M %S %p %P %u %w %a %A %b %B %h %Z %z %:z %::z",
 "%U %W %V %G %g","%Ey %EY %Od %Om %OH %Ec %EX","%-e|%0e|%_e|%-k|%0k|%-l|%0l",
 "%^a %#A %^Z %#p","%10a|%-10a|%010d","%%","%n%t"].each { |f| p t.strftime(f) }
["%","%Y%","%-","abc%","%E","%O"].each { |f| begin; p t.strftime(f); rescue => e; p [e.class, e.message]; end }
require "date"
d = Date.new(2024,2,29)
["%c","%x","%v","%Ey %Od","%","%Y%","%L","%N","%s","%Q","%+"].each { |f| p d.strftime(f) }
__END__
"Thu Feb 29 05:07:09 2024"
"02/29/24"
"05:07:09"
"02/29/24"
"2024-02-29"
"05:07:09"
"05:07"
"05:07:09 AM"
"29-FEB-2024"
"1709183229"
"123"
"123"
"123456"
"123456000"
"123456000"
"123"
"2024 20 24 02 29 29 060 05  5 05  5 07 09 AM am 4 4 Thu Thursday Feb February Feb UTC +0000 +00:00 +00:00:00"
"08 09 09 2024 24"
"24 2024 29 02 05 Thu Feb 29 05:07:09 2024 05:07:09"
"29|29|29|5|05|5|05"
"THU THURSDAY UTC am"
"       Thu|Thu|0000000029"
"%"
"\n\t"
[ArgumentError, "invalid format: %"]
[ArgumentError, "invalid format: %Y%"]
[ArgumentError, "invalid format: %-"]
[ArgumentError, "invalid format: abc%"]
"%E"
"%O"
"Thu Feb 29 00:00:00 2024"
"02/29/24"
"29-FEB-2024"
"24 29"
"%"
"2024%"
"000"
"000000000"
"1709164800"
"1709164800000"
"Thu Feb 29 00:00:00 +00:00 2024"

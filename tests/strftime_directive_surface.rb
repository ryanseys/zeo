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

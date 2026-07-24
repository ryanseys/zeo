# printf with a %b/%B binary conversion routes through the Ruby formatter
# (the String#% / format engine): C printf has no portable %b -- glibc
# 2.35+ renders it natively (which masked this on Linux), macOS prints a
# literal b. Mixed directives and width/flag forms included.
printf("%b\n", 10)
printf("%08b|\n", 5)
printf("%#b %B\n", 5, 5)
printf("x=%d b=%b s=%s\n", 7, 7, "ok")
p format("%b", 255)
p sprintf("%09b", 6)

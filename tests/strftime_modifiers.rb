t001 = Time.utc(2024, 2, 29, 5, 7, 9, 123456)
p t001.strftime("%Ey %EY %Od %Om %OH")
p t001.strftime("%6L")
p t001.strftime("%9L")
p t001.strftime("%-e|%0e|%_e|%-k|%0k|%-l|%0l")
r001 = (t001.strftime("%") rescue $!.class); p r001
r002 = (t001.strftime("%Y%") rescue $!.class); p r002
p t001.strftime("%Y-%m-%d %H:%M:%S")
p t001.strftime("%L")

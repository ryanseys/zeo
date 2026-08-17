e001 = [1, 2].each
r001 = loop { e001.next }
p r001
e002 = (1..3).each
r002 = loop { e002.next }
p r002
r003 = loop { break 5 }
p r003

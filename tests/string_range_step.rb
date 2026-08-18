r001 = []; ("a".."e").step(2) { |s001| r001 << s001 }; p r001
sr = ("a".."e")
p sr.step(2).to_a
p sr.step(2).class
p (sr % 2).to_a
p ("a".."e").to_a
r2 = []; (("a".."e") % 2).each { |s| r2 << s }; p r2

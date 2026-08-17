z001 = []; "1-2-3".each_line("-") { |l001| z001 << l001 }; p z001
p("1-2-3".each_line("-").to_a)
z2 = []; "a\nb\n".each_line { |l| z2 << l }; p z2

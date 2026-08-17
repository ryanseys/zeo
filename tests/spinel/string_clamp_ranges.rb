p("z".clamp(.."c"))
p("a".clamp("b"..))
r001 = ("b".clamp("a"..."c") rescue $!.class); p r001
r002 = ("b".clamp("c", "a") rescue $!.class); p r002
p("b".clamp("a", "c"))
p("z".clamp("b", "d"))
p("b".clamp("a".."c"))

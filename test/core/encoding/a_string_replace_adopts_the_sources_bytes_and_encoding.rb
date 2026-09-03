# ruby's `rb_str_replace` copies the source's bytes AND encoding onto the
# receiver -- no transcoding, whatever encoding the receiver was born with.
b = +"seed"
b.replace("h\xC3\xA9".b)
p [b.encoding.to_s, b.bytes]

x = +"seed"
x.replace("hi".b)
p [x.encoding.to_s, x.bytes]

u = +"".b
u.replace("héllo")
p [u.encoding.to_s, u.bytesize]

f = (+"seed").replace("h\xC3\xA9".b)
p [f.force_encoding("UTF-8"), f.bytes]
__END__
["ASCII-8BIT", [104, 195, 169]]
["ASCII-8BIT", [104, 105]]
["UTF-8", 6]
["hé", [104, 195, 169]]

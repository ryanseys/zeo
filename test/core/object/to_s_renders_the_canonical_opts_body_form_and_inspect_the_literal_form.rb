r = /abc/x
puts r.to_s
r2 = /a.c/im
puts r2.to_s
puts(/abc/x.inspect)
puts(/abc/mi.inspect)
__END__
(?x-mi:abc)
(?mi-x:a.c)
/abc/x
/abc/mi

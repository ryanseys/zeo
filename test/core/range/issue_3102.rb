p(("a".."e").count { |s| s < "c" })
p(("a".."e").count)
p(("a".."e").count("b"))
p((1..5).count { |i| i.even? })
__END__
2
5
1
2

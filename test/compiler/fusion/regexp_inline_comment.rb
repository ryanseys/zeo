# (?#comment) groups parse as empty atoms.
p(/foo(?#comment)bar/.match("foobar") ? "m" : "n")
p(/foo(?#)bar/ =~ "xfoobar")
__END__
"m"
1

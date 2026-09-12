# `&:encoding` is the only mention of Encoding in this program.
p "a\nb".lines.map(&:encoding).map(&:to_s)
p [1, 2].map(&:to_s)
__END__
["UTF-8", "UTF-8"]
["1", "2"]

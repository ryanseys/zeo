# Inclusive and exclusive, and a two-character range.
# (spinel issue #3070)
puts ("a".."e").count
puts ("a"..."e").count
puts ("aa".."ad").count
__END__
5
4
4

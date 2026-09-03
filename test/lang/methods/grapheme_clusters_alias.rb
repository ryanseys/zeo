# grapheme_clusters / each_grapheme_cluster alias to the chars/each_char
# machinery -- equivalent over the supported text domain (no combining
# sequences).
p "hello".grapheme_clusters
acc = []
"abc".each_grapheme_cluster { |ch| acc << ch }
p acc
p "ab".each_grapheme_cluster.to_a
__END__
["h", "e", "l", "l", "o"]
["a", "b", "c"]
["a", "b"]

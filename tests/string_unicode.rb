# String grapheme clusters and Unicode normalization.

# Grapheme clusters group a base character with its combining marks, and treat
# regional-indicator flags and ZWJ emoji sequences as single clusters.
combined = "áé"          # a + combining acute, e + combining acute
p combined.grapheme_clusters
p combined.grapheme_clusters.length
p "café".grapheme_clusters.length
p "🇯🇵👨‍👩‍👧".grapheme_clusters

# each_grapheme_cluster yields each cluster; blockless gives an Enumerator.
out = []
combined.each_grapheme_cluster { |g| out << g }
p out
p combined.each_grapheme_cluster.to_a

# Normalization: NFC composes, NFD decomposes, NFKC/NFKD also fold compatibility.
decomposed = "é"                # e + combining acute
p decomposed.unicode_normalize.bytes  # default :nfc -> single é codepoint
p decomposed.unicode_normalize(:nfd).length
p "ﬁ".unicode_normalize(:nfkc)         # fi ligature -> "fi"
p "①".unicode_normalize(:nfkd)         # circled 1 -> "1"

# unicode_normalized? tests membership in a form.
p "é".unicode_normalized?              # composed, already NFC
p decomposed.unicode_normalized?       # decomposed, not NFC
p decomposed.unicode_normalized?(:nfd) # ...but is NFD

# An unknown form raises ArgumentError.
begin
  "x".unicode_normalize(:bogus)
rescue ArgumentError => e
  puts e.message
end

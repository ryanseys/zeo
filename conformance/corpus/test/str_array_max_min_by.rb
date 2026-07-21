# Issue #863: max_by / min_by on String arrays return the element,
# not the score (and not 0).
puts ["hello", "hi", "goodbye"].max_by { |s| s.length }
puts ["hello", "hi", "goodbye"].min_by { |s| s.length }
# Negation flips the ordering.
puts ["a", "bb", "ccc"].max_by { |s| -s.length }

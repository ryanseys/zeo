# Issue #909. Unanchored class-sequence patterns (e.g. /\w\w/, /(.)(.)/ )
# used to skip position 0 because a later-starting Pike VM thread
# overwrote the earlier-starting match's captures. Now the MATCH
# update picks the best match per Ruby semantics: earliest start
# wins, longest-at-same-start wins.
puts ("abc" =~ /\w\w/).inspect
puts ("abc def" =~ /\w\w/).inspect
puts "abc def".scan(/(.)(.)/).inspect

# Greedy still works.
puts ("aaabbb" =~ /a+/).inspect
puts "aaabbb".scan(/a+/).inspect
puts "aaa-bbb-ccc".scan(/\w+/).inspect

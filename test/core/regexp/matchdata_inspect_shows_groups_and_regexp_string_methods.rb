p "hello".match(/l(l)o/)
p "2024-01".match(/(?<y>\d+)-(?<m>\d+)/)
p "hello".start_with?(/he/)
p "hello".start_with?(/ell/)
p "hello".byteindex(/l+/)
p "hello".byterindex(/l+/)
__END__
#<MatchData "llo" 1:"l">
#<MatchData "2024-01" y:"2024" m:"01">
true
false
2
3

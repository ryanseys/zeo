r = (1..3)
puts r.to_s
puts r.inspect
puts (1...5).to_s
puts ('a'..'c').inspect
puts ('a'...'d').to_s
puts (1..).to_s
puts (..5).inspect
__END__
1..3
1..3
1...5
"a".."c"
a...d
1..
..5

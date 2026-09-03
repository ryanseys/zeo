word = "wor"
r = /#{word}ld/
puts r.match?("world")
puts r.source
__END__
true
world

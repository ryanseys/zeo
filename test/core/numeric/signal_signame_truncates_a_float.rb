# A Float signal number is truncated toward zero before the lookup.
p Signal.signame(2.9)
p Signal.signame(2)
p Signal.signame(9.99)
__END__
"INT"
"INT"
"KILL"

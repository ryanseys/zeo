# An interpolation holding several statements runs every one of them in source
# order, for their effects on locals, and interpolates the LAST one's value.

y = 0
s = "#{y = 10; y = 20; y}"
puts s
puts y

# Side effect persists outside the interpolation.
counter = 0
"#{counter += 1; counter += 1; counter += 1}"
puts counter
__END__
20
20
3

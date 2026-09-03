# A negative epoch floors: second -1 plus a POSITIVE sub-second remainder,
# never second 0 minus half.

p Time.at(-0.5).to_i
p Time.at(-0.5).nsec
p Time.at(-1).to_i
__END__
-1
500000000
-1

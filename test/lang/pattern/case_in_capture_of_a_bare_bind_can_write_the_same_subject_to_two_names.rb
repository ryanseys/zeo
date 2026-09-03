# A real latent hazard this test guards against: `Capture(Bind(x), y)`
# writes the SAME scrutinee into two different locals -- if either write
# MOVED instead of CLONED the scrutinee, the second write would fail to
# compile ("use of moved value"). See `emit_pattern_match`'s docs.

case 5
in x => y
  puts x
  puts y
end
__END__
5
5

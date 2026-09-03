# ARGF has its own pseudo-IO class; with no file args its filename is "-".
puts ARGF.class
puts ARGF.filename
__END__
ARGF.class
-

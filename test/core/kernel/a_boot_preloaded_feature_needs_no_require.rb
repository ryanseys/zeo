# `Monitor` needs NO require: ruby 4.0 loads `monitor` before the program's
# first line, so its constant is there whatever the program says
# (oracle-verified). The ext gate is about a feature ruby does NOT preload;
# `Base64` is the shape that test belongs to, and it lives beside the
# positional-require golden.

p Monitor.new.class
__END__
Monitor

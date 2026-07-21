# Issue #868: Regexp#source returns the original pattern string;
# #options returns the CRuby flag bitmask (1=IGNORECASE, 2=EXTENDED,
# 4=MULTILINE).
# named_captures deferred -- would need a Hash + per-named-group
# index array which the runtime doesn't expose yet.
puts /hello/.source
puts /hello/i.options
puts /hello/.options
puts /world/m.options
puts /pat/x.options

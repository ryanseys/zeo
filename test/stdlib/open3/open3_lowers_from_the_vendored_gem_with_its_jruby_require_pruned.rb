# open3's last line is `require 'open3/jruby_windows' if RUBY_ENGINE ==
# 'jruby' && ...`; that file is JRuby-native (`require 'jruby'`, java_import)
# and must be pruned rather than spliced. Loading open3 and reflecting on
# its API needs no subprocess, so this exercises the load path end-to-end.

require "open3"
puts Open3.respond_to?(:capture3)
puts Open3.respond_to?(:popen3)
__END__
true
true

require "tmpdir"

p $LOAD_PATH.respond_to?(:resolve_feature_path)
# Ruby puts it on THIS array, not on Array -- a plain array must not answer.
p [].respond_to?(:resolve_feature_path)
p $LOAD_PATH.singleton_methods.sort

# A real file on a load-path root: the kind is :rb and the path is absolute.
Dir.mktmpdir do |dir|
  File.write(File.join(dir, "zeo_probe_feature.rb"), "ZEO_PROBE_FEATURE = 1\n")
  $LOAD_PATH.unshift(dir)
  r = $LOAD_PATH.resolve_feature_path("zeo_probe_feature")
  # Absolute and REAL: the tmpdir is a symlink on macOS, so the answer is the
  # resolved path rather than the one that was unshifted.
  p [r[0], File.basename(r[1]), r[1].start_with?("/"), r[1] == File.realpath(File.join(dir, "zeo_probe_feature.rb"))]
  # The explicit `.rb` spelling names the same file.
  r = $LOAD_PATH.resolve_feature_path("zeo_probe_feature.rb")
  p [r[0], File.basename(r[1])]
  $LOAD_PATH.delete(dir)
end

# Nothing supplies it: nil, not a raise.
p $LOAD_PATH.resolve_feature_path("no_such_lib_xyz")

# Ruby folded these into core and keeps the name only so old code loads. It
# has no file for them anywhere, so the answer is nil.
%w[set fiber thread rational complex].each do |f|
  p [f, $LOAD_PATH.resolve_feature_path(f)]
end

# Anything but a String is a TypeError, and nil is NOT the no-argument case.
[nil, 42, :json].each do |bad|
  begin
    $LOAD_PATH.resolve_feature_path(bad)
    p :no_raise
  rescue TypeError => e
    p e.class
  end
end

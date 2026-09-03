# DELIBERATE DIVERGENCE. zeo is a ruby built --with-static-linked-ext: every
# library here is compiled into the program, so there is no file on disk for
# resolve_feature_path to name. It answers [:so, <feature>], which is CRuby's
# own answer for a statically linked extension.
#
# The oracle is a shared-library build, so it names a real file instead, and
# splits :rb from :so by how that particular library happens to ship --
# `tmpdir` is tmpdir.rb there and `io/console` is console.bundle. Neither
# spelling is available to zeo, and inventing a path would be worse than
# naming the feature.
#
# What zeo does NOT diverge on is the nil set: a library ruby folded into core
# and keeps no file for answers nil under both. See
# `resolve_feature_path_answers_where_a_require_would_land.rb`.
%w[json psych stringio digest openssl io/console io/wait tmpdir time pathname
   monitor objspace random/formatter].each do |f|
  p [f, $LOAD_PATH.resolve_feature_path(f)]
end

# The suffix spellings collapse onto the same feature, as they do for require.
p $LOAD_PATH.resolve_feature_path("json.rb")
p $LOAD_PATH.resolve_feature_path("json.so")
__END__
["json", [:so, "json"]]
["psych", [:so, "psych"]]
["stringio", [:so, "stringio"]]
["digest", [:so, "digest"]]
["openssl", [:so, "openssl"]]
["io/console", [:so, "io/console"]]
["io/wait", [:so, "io/wait"]]
["tmpdir", [:so, "tmpdir"]]
["time", [:so, "time"]]
["pathname", [:so, "pathname"]]
["monitor", [:so, "monitor"]]
["objspace", [:so, "objspace"]]
["random/formatter", [:so, "random/formatter"]]
[:so, "json"]
[:so, "json"]

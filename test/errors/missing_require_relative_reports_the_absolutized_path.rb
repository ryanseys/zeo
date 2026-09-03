# CRuby names the absolutized path WITHOUT the extension it tried:
# `require_relative "nope"` from /tmp says `... -- /tmp/nope`.

require_relative "missing_require_relative_reports_the_absolutized_path/main"
__END__
#@ stderr
zeo::lower

  × errors/missing_require_relative_reports_the_absolutized_path/main.rb: cannot load such file -- errors/missing_require_relative_reports_the_absolutized_path/nope
  help: zeo can't compile this yet

#@ exit 1

# `host_os`/`arch` are the BUILD host's, so pinning them pinned this
# test to one machine: it read `darwin25`/`arm64-darwin25`, which is
# wrong on Linux and goes stale on the next macOS release. What the
# shim actually owes is CRuby's own agreement rule: `arch` is
# `<cpu>-<host_os>` with any `-gnu` suffix DROPPED -- a glibc ruby
# reports `host_os` `linux-gnu` beside `arch` `aarch64-linux` -- and
# both name the platform this test is running on.

require "rbconfig"
puts RbConfig::CONFIG["ruby_version"]
puts RbConfig::CONFIG.fetch("host_os").sub(/\d+\z/, "N")
puts RbConfig::CONFIG["EXEEXT"].inspect
puts RbConfig::CONFIG["arch"].sub(/\d+\z/, "N")
puts defined?(RbConfig::CONFIG)
__END__
4.0.0
darwinN
""
arm64-darwinN
constant

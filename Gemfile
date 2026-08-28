# frozen_string_literal: true

# Every gem version zeo depends on, in the one format ruby already reads.
#
# The same lock answers two questions that used to be answered separately,
# and therefore differently:
#
#   * what the compiler VENDORS as its bundled-gem set, and
#   * what the ruby ORACLE resolves when a golden is blessed against it.
#
# When those two drifted apart a golden recorded a difference between two
# library versions and called it a zeo bug. `json` is the case that forced
# this: zeo shipped 2.21.2's parser while the oracle resolved whatever the
# machine had, and the two disagree about a line counter.
#
# No `ruby` directive. The oracle's ruby is pinned in `mise.toml` (4.0.6);
# the Linux container deliberately runs the distro's ruby for `tools/zeo-dev`
# and would refuse a lock that named a version it does not have.

source "https://rubygems.org"

# --- What zeo vendors -----------------------------------------------------
#
# One line per `gems/<name>/`, at the version that directory's gemspec
# claims. Four of them are spelled differently here than on disk:
#
#   gems/English   -> `english`, which is the name ruby's own
#                     lib/English.gemspec gives it. zeo's copy says
#                     "English" and is simply wrong.
#   gems/rubygems  -> `rubygems-update`, the only published gem carrying the
#                     `lib/rubygems/**` tree. Its require_paths deliberately
#                     is not `lib`, so installing it cannot shadow the
#                     running RubyGems.
#   gems/bundler   -> no line of its own. `rubygems-update` ships the whole
#                     `bundler/` subtree at the same version, which is the
#                     same one-tag-two-gems shape upstream.rb already used.
#                     A `gem "bundler"` line would additionally force every
#                     contributor to run exactly that bundler.
#   gems/socket, gems/pty, gems/monitor
#                  -> absent. ruby 4.0.6 carries these as plain ext/lib with
#                     no gemspec anywhere, and no repository publishes them,
#                     so there is nothing to pin. Their Ruby halves are
#                     zeo-authored and zeo versions them itself.

gem "abbrev", "0.1.2"
gem "benchmark", "0.5.0"
gem "bigdecimal", "4.1.2"
gem "csv", "3.3.6"
gem "date", "3.5.1"
gem "debug", "1.11.1"
gem "delegate", "0.6.1"
gem "did_you_mean", "2.0.0"
gem "drb", "2.2.3"
gem "english", "0.8.1"
gem "erb", "6.0.7"
gem "error_highlight", "0.7.2"
gem "ffi", "1.17.4"
gem "fiddle", "1.1.8"
gem "fileutils", "1.8.0"
gem "find", "0.2.0"
gem "forwardable", "1.4.0"
gem "getoptlong", "0.2.1"
gem "ipaddr", "1.2.9"
gem "irb", "1.18.0"
gem "json", "2.21.2"
gem "logger", "1.7.0"
gem "matrix", "0.4.3"
gem "minitest", "6.0.6"
gem "mutex_m", "0.3.0"
gem "net-ftp", "0.3.9"
gem "net-http", "0.9.1"
gem "net-imap", "0.6.6"
gem "net-pop", "0.1.2"
gem "net-protocol", "0.2.2"
gem "net-smtp", "0.5.1"
gem "nkf", "0.3.0"
gem "observer", "0.1.2"
gem "open-uri", "0.5.0"
gem "open3", "0.2.1"
gem "openssl", "4.0.2"
gem "optparse", "0.8.1"
gem "ostruct", "0.6.3"
gem "power_assert", "3.0.1"
gem "pp", "0.6.4"
gem "prettyprint", "0.2.0"
gem "prime", "0.1.4"
gem "prism", "1.9.0"
gem "pstore", "0.2.1"
gem "psych", "5.4.0"
gem "racc", "1.8.1"
gem "rake", "13.4.2"
gem "reline", "0.7.0"
gem "resolv", "0.7.1"
gem "rexml", "3.4.4"
gem "rubygems-update", "4.0.18"
gem "shellwords", "0.2.2"
gem "singleton", "0.3.0"
gem "strscan", "3.1.6"
gem "syslog", "0.4.0"
gem "tempfile", "0.3.1"
gem "test-unit", "3.7.8"
gem "time", "0.4.2"
gem "timeout", "0.6.1"
gem "tmpdir", "0.3.1"
gem "tsort", "0.2.0"
gem "un", "0.3.0"
gem "uri", "1.1.1"
gem "weakref", "0.1.4"
gem "zlib", "3.2.3"

# --- What only the oracle needs -------------------------------------------
#
# Not vendored, so no `gems/<name>/` and nothing for the equivalence check to
# compare against. A group is documentation only: bundler writes no group
# into the lock.
group :oracle do
  gem "rspec", "3.13.2"
end

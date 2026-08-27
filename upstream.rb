# frozen_string_literal: true

# zeo's vendored batteries: the git-sourced trees the project pins.
#
# `gem` entries are vendored into the committed `gems/<name>/` -- a stub
# gemspec plus the verbatim `lib/` -- which is what the compiler reads, so a
# fresh clone builds offline with no extra step. `gemtest` entries are whole
# checkouts fetched on demand into the gitignored `vendor/gemtests/<name>/`,
# because a `.gem` archive does not ship `test/`.
#
# `rev` is the reproducible pin: `zeo-dev gem sync` resolves the tag to a
# commit SHA once and every later sync reuses it. `subdir` is for a repo that
# ships more than one gem -- `rubygems/rubygems` carries bundler under
# `bundler/`.
#
# Nothing here discovers a new version on its own. `zeo-dev gem outdated` is
# the discovery half; it prints each pin beside what the ORACLE ruby installs,
# which is the version to match. Vendoring ahead of the ruby every golden is
# blessed against manufactures divergences that are not bugs.
#
# Managed by `tools/zeo-dev gem`. `upstream.lock` beside this file is the
# derived JSON everything else reads.

gem "abbrev",
  github: "ruby/abbrev",
  tag: "v0.1.2",
  rev: "b362e8e3b92c48a9e8f34fc0f782d3396858db1f"
gem "benchmark",
  github: "ruby/benchmark",
  tag: "v0.5.0",
  rev: "efa6e613cee9e3e47831c6767a1df08ac35b18cc"
gem "bundler",
  github: "rubygems/rubygems",
  tag: "v4.0.18",
  rev: "7e934435ff880a9a6f78b9ca5fea396a275bd5c4",
  subdir: "bundler"
gem "csv",
  github: "ruby/csv",
  tag: "v3.3.6",
  rev: "0873ab362d996f12796c3c3e8998b5be657a9b12"
gem "did_you_mean",
  github: "ruby/did_you_mean",
  tag: "v2.0.0",
  rev: "1cce337962d51ee90bb7ff51a51a803fc2384e0e"
gem "drb",
  github: "ruby/drb",
  tag: "v2.2.3",
  rev: "a4f74442da5ece44f99fd4ff4f0299a3efa25004"
gem "erb",
  github: "ruby/erb",
  tag: "v6.0.7",
  rev: "9907393a1f1dfc027e8e7f2a9f5fcc7c60632762"
gem "fileutils",
  github: "ruby/fileutils",
  tag: "v1.8.0",
  rev: "29de582f683bbf5d8d153599459f2b44372ac7b4"
gem "find",
  github: "ruby/find",
  tag: "v0.2.0",
  rev: "192237e766c191fbba4578fafbad94f4c9d3d3a4"
gem "ipaddr",
  github: "ruby/ipaddr",
  tag: "v1.2.9",
  rev: "f17f68bcab9cfb256cceac4546bf4e11876b2aa8"
gem "logger",
  github: "ruby/logger",
  tag: "v1.7.0",
  rev: "f474d07d9890a03e6e40430c4e2ee933c6193d7e"
gem "net-ftp",
  github: "ruby/net-ftp",
  tag: "v0.3.9",
  rev: "4d48bec568251d0a94190b7508106fb8ca4b7c1c"
gem "net-http",
  github: "ruby/net-http",
  tag: "v0.9.1",
  rev: "8cee86e939f69bd0906864e7eb740bb471a205bd"
gem "net-protocol",
  github: "ruby/net-protocol",
  tag: "v0.2.2",
  rev: "2d3c4b43a837a616e5853f807cde63aaffbcd280"
gem "net-smtp",
  github: "ruby/net-smtp",
  tag: "v0.5.1",
  rev: "a0075eb8a74910bd05566f0a208589263bce7597"
gem "observer",
  github: "ruby/observer",
  tag: "v0.1.2",
  rev: "6c978e6196b33405aced08ca3c3a5600f5b271e5"
gem "open3",
  github: "ruby/open3",
  tag: "v0.2.1",
  rev: "b8909222051b4103a19eba19506727faece252e7"
gem "power_assert",
  github: "ruby/power_assert",
  tag: "v3.0.1",
  rev: "26edf65c9932de6062606766878cf57a453af300"
gem "racc",
  github: "ruby/racc",
  tag: "v1.8.1",
  rev: "5229883dca5b451c8bfd322272ccd2ca6d526695"
gem "resolv",
  github: "ruby/resolv",
  tag: "v0.7.1",
  rev: "8fc05c1cb6cd36e4a8d0391aeb8cacdcb2368b4c"
gem "rubygems",
  github: "rubygems/rubygems",
  tag: "v4.0.18",
  rev: "7e934435ff880a9a6f78b9ca5fea396a275bd5c4"
gem "tempfile",
  github: "ruby/tempfile",
  tag: "v0.3.1",
  rev: "297bdf2c8d959d4455360134152d33d2bea8bf54"
gem "test-unit",
  github: "test-unit/test-unit",
  tag: "3.7.8",
  rev: "34a125ca2fd83552f2e8bd8000036893372d2c43"
gem "time",
  github: "ruby/time",
  tag: "v0.4.2",
  rev: "387292f5d2639e7100c2118254047514068da6e4"
gem "tmpdir",
  github: "ruby/tmpdir",
  tag: "v0.3.1",
  rev: "0245079c2489f9806f8556aa74e01540adaa3278"
gem "un",
  github: "ruby/un",
  tag: "v0.3.0",
  rev: "1f636a623914bc19c3ad441ca1eea1abaabaa1b7"
gem "uri",
  github: "ruby/uri",
  tag: "v1.1.1",
  rev: "f1b05c89ab38667e7564896f994d4d6cfbc67149"
gemtest "rack",
  github: "rack/rack",
  tag: "v3.2.6",
  rev: "e1f22fdbe99afd2126b6fbf05bb12399359574b7"
gemtest "msgpack",
  github: "msgpack/msgpack-ruby",
  tag: "v1.8.4",
  rev: "42378b091b0936d27bc9649da366e90798345fe6"

# MRI's public C API headers. zeo compiles a gem's `ext/**/*.c` from source
# against these, so a prebuilt MRI `.so` never loads. The vendored tree is
# upstream verbatim plus `crates/zeo-rt/cext/patches/`, which turns the
# layout-reading macros into calls; `zeo-dev cext sync --check` proves the
# tree is exactly that sum.
headers "ruby",
  github: "ruby/ruby",
  tag: "v4.0.6",
  rev: "03b6d3f8898a28604fe6cb00eae3226b821168f4",
  subdir: "include"
gem "error_highlight",
  github: "ruby/error_highlight",
  tag: "v0.7.2",
  rev: "d945ffd2aa4e6c36665195b6a5db8a73ac5339af"
gem "rake",
  github: "ruby/rake",
  tag: "v13.4.2",
  rev: "503b8ec593c51289c09cc2a69a34af99d6198c6a"
gem "open-uri",
  github: "ruby/open-uri",
  tag: "v0.5.0",
  rev: "8f5a4ef6f91692cc1833d2b59fd4d2609526eb34"
gem "rexml",
  github: "ruby/rexml",
  tag: "v3.4.4",
  rev: "4f32ea33bc3f71cced67487659beef58edcf6d56"
gem "debug",
  github: "ruby/debug",
  tag: "v1.11.1",
  rev: "bad4d38f8330219b62f2b253d59146f5a71fd39a"
gem "matrix",
  github: "ruby/matrix",
  tag: "v0.4.3",
  rev: "de06454b6c80e83b98890d433b64422ce9bd49a9"
gem "mutex_m",
  github: "ruby/mutex_m",
  tag: "v0.3.0",
  rev: "9fc3ee4f241fef210d2da125f4944d82219d8d4b"
gem "pstore",
  github: "ruby/pstore",
  tag: "v0.2.1",
  rev: "4d71b36b82237a8c11a68c146eae5191544dd304"
gem "getoptlong",
  github: "ruby/getoptlong",
  tag: "v0.2.1",
  rev: "f49629dfaa17b2dbb8c776ff9bf52ef8a050c19a"
gem "net-imap",
  github: "ruby/net-imap",
  tag: "v0.6.6",
  rev: "7cc1dd0a11a4f5faeb5c92676f78e348a0104a87"
gem "net-pop",
  github: "ruby/net-pop",
  tag: "v0.1.2",
  rev: "0ed5794e8eaa8f4a4f1f402936ee56ca8ef89e0d"
gem "reline",
  github: "ruby/reline",
  tag: "v0.7.0",
  rev: "841ab2f88fc1d190873f19f151e3fb88772d30e7"

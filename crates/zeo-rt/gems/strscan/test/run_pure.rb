# frozen_string_literal: true

# Runs the vendored upstream strscan suite (test_stringscanner.rb, verbatim
# from ruby/strscan at v3.1.8) against the PURE port in ../lib, under a real
# ruby. The pure tree must be required before bundler/setup runs, or the
# store's C strscan lands ahead of it on $LOAD_PATH.
$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
require "strscan"
loaded = $LOADED_FEATURES.grep(/strscan/)
unless loaded.length == 1 && loaded[0].end_with?("gems/strscan/lib/strscan.rb")
  raise "the pure tree did not win the require: #{loaded.inspect}"
end
require "bundler/setup"
require_relative "test_stringscanner"

# Implementation-specific: asserts the scanner calls NO methods on the
# subject String, which is true of the C extension and impossible for a
# pure port (TruffleRuby omits it for the same reason).
StringScannerTests.send(:remove_method, :test_charpos_not_use_string_methods)

# frozen_string_literal: true

# Runs the vendored upstream stringio suite (test_stringio.rb, verbatim from
# ruby/stringio at v3.2.0) against the PURE port in ../lib, under a real
# ruby. The pure tree must be required before bundler/setup runs, or the
# store's C stringio lands ahead of it on $LOAD_PATH.
$LOAD_PATH.unshift File.expand_path("../lib", __dir__)
require "stringio"
loaded = $LOADED_FEATURES.grep(/stringio/)
unless loaded.length == 1 && loaded[0].end_with?("gems/stringio/lib/stringio.rb")
  raise "the pure tree did not win the require: #{loaded.inspect}"
end
require "bundler/setup"
require_relative "test_stringio"

# Pull in the statically linked native half FIRST, so StringScanner exists to
# be reopened below. This is CRuby's loader idiom -- `ext/digest/lib/digest.rb`
# opens with `require 'digest/loader'`, which is just `require 'digest.so'`.
require "strscan.so"

class StringScanner
  # `strscan.c`'s own constants. rexml gates two compat shims on
  # `StringScanner::Version` (`if StringScanner::Version < "3.0.8"`), so their
  # absence was a NameError at load, not merely a reflection gap. Kept in step
  # with the gemspec version by `gems/UPSTREAM.md`.
  Version = "3.1.6"
  Id = "$Id$"

  # CRuby defines this in C (`strscan.c`), but a feature-gated native class
  # cannot register a constructible exception in this runtime: an ABI row is
  # gated-but-constructor-less, and the exception table is constructible-but-
  # ungated, so no row shape is both. Defining it here makes it an ordinary
  # user class -- registered under its fully qualified name with a real
  # constructor -- which the native half then raises by name.
  class Error < StandardError; end
end

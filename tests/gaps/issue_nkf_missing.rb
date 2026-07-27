# nkf is a CRuby C extension (Network Kanji Filter, Japanese text encoding
# conversion) that zeo has no native implementation of -- `require "nkf"`
# raises LoadError.
require "nkf"
p defined?(NKF)

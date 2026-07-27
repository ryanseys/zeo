# fcntl is a CRuby C extension (fcntl(2)/open(2) flag constants, e.g.
# Fcntl::F_GETFL) that zeo has no native implementation of -- `require
# "fcntl"` raises LoadError.
require "fcntl"
p Fcntl::F_GETFL

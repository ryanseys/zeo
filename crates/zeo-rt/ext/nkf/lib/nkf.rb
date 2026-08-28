# CRuby's loader idiom: the gem's Ruby entry point pulls in the native
# half. (Upstream also branches for JRuby here; zeo is always the
# statically linked extension.)
require 'nkf.so'

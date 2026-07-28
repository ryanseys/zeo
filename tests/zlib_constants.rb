# `require "zlib"` brings up the whole module: the 38 constants, the six
# stream classes, and the thirteen exception classes the gem's Ruby half
# defines. This is the shape check -- behaviour is in zlib_classes.rb.
require "zlib"
p Zlib.const_defined?(:Deflate)
p Zlib.constants.sort
p Zlib::GzipFile.constants.sort
p [Zlib::Deflate.superclass, Zlib::Inflate.superclass, Zlib::GzipReader.superclass]
p Zlib::GzipReader.ancestors.include?(Enumerable)
p [Zlib::BufError.superclass, Zlib::Error.superclass, Zlib::GzipFile::CRCError.superclass]

# require "zlib" only defines the bare Zlib module -- none of its actual
# functionality (Deflate/Inflate, GzipWriter/GzipReader, error classes,
# constants like BEST_COMPRESSION) is implemented, so it's unusable for
# real compression/decompression work.
require "zlib"
p Zlib.const_defined?(:Deflate)
p Zlib.constants.sort

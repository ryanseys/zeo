# Pull in the statically linked native half FIRST, so `Zlib` and its stream
# classes exist to be reopened below. This is CRuby's loader idiom -- see
# `ext/strscan/lib/strscan.rb` for the same shape and the reason for it.
require "zlib.so"

module Zlib
  # CRuby defines these in C (`ext/zlib/zlib.c`), but a feature-gated native
  # class cannot register a constructible exception in this runtime: an ABI row
  # is gated-but-constructor-less, and the exception table is constructible-but-
  # ungated, so no row shape is both. Defined here they are ordinary user
  # classes -- registered under their fully qualified names with real
  # constructors -- which the native half raises by name.
  class Error < StandardError; end

  # One per zlib return code, so a caller can rescue the specific failure.
  # `StreamEnd`/`NeedDict` are the two that are not failures at the C level;
  # CRuby still models them as exceptions and so does this.
  class StreamEnd < Error; end
  class NeedDict < Error; end
  class DataError < Error; end
  class StreamError < Error; end
  class MemError < Error; end
  class BufError < Error; end
  class VersionError < Error; end

  # Raised when a method is called on a stream that is mid-operation.
  class InProgressError < Error; end

  class GzipFile
    # The gzip errors carry the bytes read so far, so a caller that hits a
    # truncated or corrupt member can still see what arrived. The native half
    # constructs these through `raise` with a message and then assigns
    # `@input`, which is why the reader is here and the writer is not.
    class Error < Zlib::Error
      attr_reader :input

      def inspect
        "#<#{self.class}: #{message}, input=#{@input.inspect}>"
      end
    end

    # The member ended before its 8-byte footer arrived.
    class NoFooter < Error; end
    # The footer's CRC-32 disagrees with the decompressed bytes.
    class CRCError < Error; end
    # The footer's ISIZE disagrees with the decompressed length.
    class LengthError < Error; end
  end
end

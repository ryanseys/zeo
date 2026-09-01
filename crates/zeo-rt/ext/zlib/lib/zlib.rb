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

  # The gzip container (RFC 1952) over the native raw-deflate streams:
  # `GzipFile` holds the header fields and lifecycle both directions share,
  # `GzipWriter` compresses into an IO, `GzipReader` decompresses out of one.
  # Only the codec is native (`Zlib::Deflate`/`Zlib::Inflate` with negative
  # window bits); the framing, the CRC bookkeeping and the IO surface are
  # this Ruby.
  class GzipFile
    # The gzip errors carry the bytes read so far, so a caller that hits a
    # truncated or corrupt member can still see what arrived.
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

    # CRuby's `GzipFile` registers no allocator; only the subclasses
    # construct.
    def self.new(*args, &block)
      raise TypeError, "allocator undefined for Zlib::GzipFile" if self == GzipFile
      super
    end

    def self.wrap(io, *rest, &block)
      __finish_after(new(io, *rest), block)
    end

    # The block form's contract: the stream is finished however the block
    # leaves, and the block's own failure wins over any close failure.
    def self.__finish_after(gz, block)
      return gz unless block
      begin
        result = block.call(gz)
      rescue Exception => e
        begin
          gz.__shut_down(true)
        rescue Exception
        end
        raise e
      end
      gz.__shut_down(true)
      result
    end

    # `#close` shuts the member down and closes the IO underneath if this
    # stream opened it; `#finish` leaves the IO alone. Both answer the IO --
    # which is how `GzipWriter.open(path) { ... }` hands back the File.
    def close
      __shut_down(true)
    end

    def finish
      __shut_down(false)
    end

    def closed?
      @gz_closed
    end

    def __shut_down(close_io)
      unless @gz_closed
        __finish_member
        @gz_closed = true
        @gz_io.close if close_io && @gz_own_io
      end
      @gz_io
    end

    def __own_io!
      @gz_own_io = true
    end

    # Copying a stream would interleave two readers or writers over one IO,
    # so a copy arrives already closed -- CRuby's dup fails the same way.
    def initialize_copy(other)
      @gz_closed = true
      self
    end

    def to_io
      __check_open
      @gz_io
    end

    def orig_name
      __check_open
      @gz_orig_name && @gz_orig_name.dup
    end

    def comment
      __check_open
      @gz_comment && @gz_comment.dup
    end

    # A gzip header records only the two extreme compression levels, so
    # everything between reads back as `DEFAULT_COMPRESSION`.
    def level
      __check_open
      case @gz_xfl
      when 2 then Zlib::BEST_COMPRESSION
      when 4 then Zlib::BEST_SPEED
      else Zlib::DEFAULT_COMPRESSION
      end
    end

    def os_code
      __check_open
      @gz_os
    end

    def mtime
      __check_open
      Time.at(@gz_mtime)
    end

    # The running CRC-32 of the uncompressed bytes -- the value that goes into
    # the footer, and that a reader has verified once it reaches the end.
    def crc
      __check_open
      @gz_crc
    end

    # Every write goes straight through to the IO, so `sync` is already the
    # behaviour `sync = true` asks for; the flag is recorded and reported.
    def sync
      __check_open
      @gz_sync
    end

    def sync=(mode)
      __check_open
      @gz_sync = mode ? true : false
      mode
    end

    private

    def __gz_init(io)
      @gz_io = io
      @gz_closed = false
      @gz_own_io = false
      @gz_sync = false
      @gz_crc = 0
      @gz_size = 0
      @gz_orig_name = nil
      @gz_comment = nil
    end

    def __check_open
      raise GzipFile::Error, "closed gzip stream" if @gz_closed
    end

    # Only a HEADER-phase failure carries the bytes read so far; CRuby leaves
    # `input` nil on footer and mid-stream errors.
    def __gz_raise(klass, message, input = nil)
      e = klass.new(message)
      e.instance_variable_set(:@input, input && input.dup)
      raise e
    end
  end

  # An IO-shaped gzip compressor. Every write goes straight through to the
  # underlying IO, so a caller that never closes the writer still gets a
  # valid prefix on disk; `#close` adds the deflate tail and the footer that
  # make it a complete member.
  class GzipWriter < GzipFile
    def initialize(io, level = nil, strategy = nil)
      __gz_init(io)
      @deflate = Zlib::Deflate.new(level, -Zlib::MAX_WBITS, nil, strategy)
      @gz_mtime = Time.now.to_i
      @gz_xfl =
        case level
        when Zlib::BEST_COMPRESSION then 2
        when Zlib::BEST_SPEED then 4
        else 0
        end
      @gz_os = Zlib::OS_CODE
      @header_written = false
    end

    # `GzipWriter.open(path, level = nil) { |gz| ... }` -- opens the file
    # itself, so `#close` closes it, and closes it even if the block raises.
    def self.open(path, level = nil, strategy = nil, &block)
      gz = new(File.new(path, "wb"), level, strategy)
      gz.__own_io!
      __finish_after(gz, block)
    end

    # `#write` answers how many bytes went IN, as `IO#write` does -- and like
    # it, accepts any number of arguments, zero included.
    def write(*args)
      total = 0
      args.each { |a| total += __write_data(__bytes_of(a)) }
      total
    end

    def <<(obj)
      __write_data(__bytes_of(obj))
      self
    end

    def print(*args)
      args.each { |a| __write_data(__bytes_of(a)) }
      nil
    end

    def printf(fmt, *args)
      __write_data(format(fmt, *args).b)
      nil
    end

    def putc(obj)
      byte = obj.is_a?(Integer) ? (obj & 0xff).chr : __bytes_of(obj).byteslice(0, 1)
      __write_data(byte)
      obj
    end

    # `#puts` follows `Kernel#puts`: no arguments is a bare newline, an Array
    # contributes each element recursively, and a line that already ends in a
    # newline is not given another.
    def puts(*args)
      if args.empty?
        __write_data("\n".b)
        return nil
      end
      args.each { |a| __puts_one(a) }
      nil
    end

    # How many UNCOMPRESSED bytes have been written -- the value that ends up
    # in the footer's length field.
    def pos
      __check_open
      @gz_size
    end
    alias tell pos

    # `#flush(flush = SYNC_FLUSH)` -- push what has been compressed so far
    # through to the IO without ending the member. An explicit nil means
    # NO_FLUSH, as CRuby reads it.
    def flush(mode = Zlib::SYNC_FLUSH)
      __check_open
      mode = Zlib::NO_FLUSH if mode.nil?
      out = +"".b
      unless @header_written
        out << __header_bytes
        @header_written = true
      end
      out << @deflate.deflate("", mode)
      @gz_io.write(out) unless out.empty?
      self
    end

    # The header fields, settable only until the header goes out -- which the
    # first write does. A late assignment would never reach the file, so it
    # raises instead of dropping.
    def mtime=(value)
      __settable
      @gz_mtime = value.to_i
      value
    end

    def orig_name=(value)
      __settable
      @gz_orig_name = __header_text(value)
      value
    end

    def comment=(value)
      __settable
      @gz_comment = __header_text(value)
      value
    end

    def __finish_member
      out = +"".b
      unless @header_written
        out << __header_bytes
        @header_written = true
      end
      out << @deflate.finish
      out << [@gz_crc, @gz_size].pack("VV")
      @gz_io.write(out)
    end

    private

    def __write_data(data)
      __check_open
      @gz_crc = Zlib.crc32(data, @gz_crc)
      @gz_size = (@gz_size + data.bytesize) & 0xffffffff
      out = +"".b
      unless @header_written
        out << __header_bytes
        @header_written = true
      end
      out << @deflate.deflate(data, Zlib::NO_FLUSH)
      @gz_io.write(out) unless out.empty?
      data.bytesize
    end

    def __header_bytes
      flg = 0
      flg |= 0x08 if @gz_orig_name
      flg |= 0x10 if @gz_comment
      out = +"\x1f\x8b\x08".b
      out << flg.chr << [@gz_mtime].pack("V") << @gz_xfl.chr << @gz_os.chr
      out << @gz_orig_name << "\0" if @gz_orig_name
      out << @gz_comment << "\0" if @gz_comment
      out
    end

    def __bytes_of(obj)
      (obj.is_a?(String) ? obj : obj.to_s).b
    end

    # A gzip header string is NUL-terminated on the wire, so it cannot
    # contain one.
    def __header_text(value)
      bytes = __bytes_of(value)
      raise GzipFile::Error, "string contains null byte" if bytes.include?("\0")
      bytes
    end

    def __settable
      __check_open
      raise GzipFile::Error, "header is already written" if @header_written
    end

    def __puts_one(value)
      if value.is_a?(Array)
        value.each { |e| __puts_one(e) }
      else
        s = __bytes_of(value)
        __write_data(s)
        __write_data("\n".b) unless s.end_with?("\n")
      end
    end
  end

  # An IO-shaped gzip decompressor, `Enumerable` over its lines.
  #
  # The member's header is read by `new`, which is why a non-gzip stream
  # fails there rather than at the first read. The footer is verified at one
  # precise moment -- when the deflate stream has ended AND every decoded
  # byte has been handed out -- so `read` (which drains in one go) reports a
  # corrupt member's data without error while `readlines` raises. That is
  # CRuby's rule, and it is directly observable.
  class GzipReader < GzipFile
    include Enumerable

    def initialize(io, *_opts)
      __gz_init(io)
      @inflate = Zlib::Inflate.new(-Zlib::MAX_WBITS)
      @gz_input = +"".b
      @gz_buf = +"".b
      @gz_pos = 0
      @gz_lineno = 0
      @stream_end = false
      @io_eof = false
      @footer_checked = false
      __read_header
    end

    def self.open(path, &block)
      gz = new(File.new(path, "rb"))
      gz.__own_io!
      __finish_after(gz, block)
    end

    # `GzipReader.zcat(io)` -- every member in the stream. Where `read` stops
    # at the first footer, this picks the next member up out of what followed
    # it. With a block, each member's data is yielded in turn.
    def self.zcat(io, &block)
      total = block ? nil : +"".b
      leftover = +"".b
      loop do
        if leftover.empty?
          head = io.read(1)
          break if head.nil? || head.empty?
          leftover = head.b
        end
        gz = new(ZCatSource.new(leftover, io))
        data = gz.read
        block ? block.call(data) : total << data.b
        leftover = (gz.unused || +"".b).b
        gz.finish
      end
      total
    end

    # `#read` with no length answers the rest of the member in the external
    # encoding; with one it answers raw bytes, and nil at the end. That split
    # is CRuby's.
    def read(length = nil, outbuf = nil)
      __check_open
      if length.nil?
        __decode_more until @stream_end
        if __check_when_drained
          data = "".force_encoding(__external)
        else
          data = __take(@gz_buf.bytesize).force_encoding(__external)
        end
        # CRuby's whole-member read ignores the buffer argument.
        return data
      end
      length = length.to_int
      raise ArgumentError, "negative length #{length} given" if length.negative?
      return +"".b if length.zero?
      __fill(length)
      return nil if __check_when_drained
      __into(outbuf, __take(length))
    end

    # `#readpartial` hands back whatever is already decoded rather than
    # waiting for `length` bytes, and reports the end as EOFError.
    def readpartial(length, outbuf = nil)
      __check_open
      __decode_more if @gz_buf.empty? && !@stream_end
      raise EOFError, "end of file reached" if __check_when_drained
      __into(outbuf, __take(length))
    end

    def gets(*args)
      __check_open
      __getline(*__line_args(args))
    end

    def readline(*args)
      gets(*args) || raise(EOFError, "end of file reached")
    end

    def readlines(*args)
      sep, limit = __line_args(args)
      lines = []
      while (line = begin
        __check_open
        __getline(sep, limit)
      end)
        lines << line
      end
      lines
    end

    def each(*args)
      return enum_for(:each) unless block_given?
      sep, limit = __line_args(args)
      loop do
        __check_open
        line = __getline(sep, limit)
        break if line.nil?
        yield line
      end
      self
    end
    alias each_line each

    def each_byte
      return enum_for(:each_byte) unless block_given?
      while (b = getbyte)
        yield b
      end
      self
    end

    def each_char
      return enum_for(:each_char) unless block_given?
      while (c = getc)
        yield c
      end
      self
    end

    # Single characters and bytes. `getc`/`getbyte` answer nil at the end,
    # `readchar`/`readbyte` raise.
    def getc
      __check_open
      first = __read_bytes(1)
      return nil if first.nil?
      want = __char_len(first.getbyte(0))
      while first.bytesize < want
        more = __read_bytes(want - first.bytesize)
        break if more.nil?
        first << more
      end
      first.force_encoding(__external)
    end

    def readchar
      getc || raise(EOFError, "end of file reached")
    end

    def getbyte
      __check_open
      b = __read_bytes(1)
      b && b.getbyte(0)
    end

    def readbyte
      getbyte || raise(EOFError, "end of file reached")
    end

    def ungetc(string)
      __check_open
      string = string.is_a?(Integer) ? string.chr : string.to_str
      __unread(string.b)
      nil
    end

    def ungetbyte(value)
      __check_open
      bytes = value.is_a?(Integer) ? (value & 0xff).chr : value.to_str.b.byteslice(0, 1)
      __unread(bytes)
      nil
    end

    # How many uncompressed bytes have been handed out; pushing bytes back
    # with `ungetc` moves it backwards, as CRuby's does.
    def pos
      __check_open
      @gz_pos
    end
    alias tell pos

    def eof?
      __check_open
      __decode_more while !@stream_end && @gz_buf.empty?
      @stream_end && @gz_buf.empty?
    end
    alias eof eof?

    def lineno
      __check_open
      @gz_lineno
    end

    def lineno=(number)
      __check_open
      @gz_lineno = number.to_int
    end

    # Bytes that followed the member's footer -- nil until it has been read.
    # Asking CONSUMES the footer, so this can raise `CRCError`, exactly as
    # CRuby's does.
    def unused
      __check_open
      return nil unless __check_when_drained
      @gz_input.dup
    end

    def external_encoding
      __check_open
      __external
    end

    # Start the member over: rewind the IO, throw the codec away, re-read the
    # header. Only a seekable IO can do this.
    def rewind
      __check_open
      @gz_io.rewind
      @inflate = Zlib::Inflate.new(-Zlib::MAX_WBITS)
      @gz_input = +"".b
      @gz_buf = +"".b
      @gz_pos = 0
      @gz_lineno = 0
      @gz_crc = 0
      @gz_size = 0
      @stream_end = false
      @io_eof = false
      @footer_checked = false
      __read_header
      0
    end

    def __finish_member
    end

    # Serves one member's leftover bytes ahead of the underlying IO, so
    # `zcat` can start the next member where `unused` left off.
    class ZCatSource
      def initialize(head, io)
        @head = head
        @io = io
      end

      def read(length)
        unless @head.empty?
          out = @head.byteslice(0, length)
          @head = @head.byteslice(out.bytesize..) || +"".b
          return out
        end
        @io.read(length)
      end
    end
    private_constant :ZCatSource

    private

    READ_CHUNK = 16 * 1024

    def __external
      Encoding.default_external
    end

    # Pull one chunk of compressed bytes off the IO. Answers whether any
    # arrived; false means the IO is spent and will not answer again.
    def __pull
      return false if @io_eof
      got = @gz_io.read(READ_CHUNK)
      if got.nil? || got.empty?
        @io_eof = true
        return false
      end
      @gz_input << got.b
      true
    end

    # Decode one round of input, pulling more from the IO if the codec wants
    # it. The native stream counts only what it consumed into `total_in`, so
    # whatever follows a finished member stays in `@gz_input`.
    def __decode_more
      return 0 if @stream_end
      if @gz_input.empty? && !__pull
        __gz_raise(GzipFile::Error, "unexpected end of file")
      end
      before = @inflate.total_in
      fresh =
        begin
          @inflate.inflate(@gz_input)
        rescue Zlib::Error
          __gz_raise(GzipFile::Error, "invalid compressed data -- format violated")
        end
      if @inflate.finished?
        @stream_end = true
        consumed = @inflate.total_in - before
        @gz_input = @gz_input.byteslice(consumed..) || +"".b
      else
        @gz_input = +"".b
      end
      @gz_buf << fresh
      @gz_crc = Zlib.crc32(fresh, @gz_crc)
      @gz_size = (@gz_size + fresh.bytesize) & 0xffffffff
      fresh.bytesize
    end

    # Verify the footer, exactly once, at the moment the stream drains --
    # every read entry point calls this after filling, and it is a no-op
    # until then. Answers whether the member is over.
    def __check_when_drained
      return false unless @stream_end && @gz_buf.empty?
      return true if @footer_checked
      __pull while @gz_input.bytesize < 8 && !@io_eof
      @footer_checked = true
      __gz_raise(NoFooter, "footer is not found") if @gz_input.bytesize < 8
      crc, isize = @gz_input.byteslice(0, 8).unpack("VV")
      @gz_input = @gz_input.byteslice(8..) || +"".b
      __gz_raise(CRCError, "invalid compressed data -- crc error") if crc != @gz_crc
      __gz_raise(LengthError, "invalid compressed data -- length error") if isize != @gz_size
      true
    end

    def __fill(want)
      __decode_more while !@stream_end && @gz_buf.bytesize < want
    end

    def __take(n)
      n = @gz_buf.bytesize if n > @gz_buf.bytesize
      out = @gz_buf.byteslice(0, n)
      @gz_buf = @gz_buf.byteslice(n..) || +"".b
      @gz_pos += n
      out
    end

    # `read_n`'s shape: fill for what the call needs, then let the drain
    # check decide whether the answer is data or the end of the member.
    def __read_bytes(want)
      __fill(want)
      return nil if __check_when_drained
      __take(want)
    end

    def __unread(bytes)
      @gz_buf = bytes + @gz_buf
      @gz_pos -= bytes.bytesize
    end

    # An EOF before ten header bytes reads as "not gzip at all"; one after
    # them is a member cut short. CRuby splits the message the same way.
    def __read_header
      loop do
        break if __try_parse_header
        next if __pull
        if @gz_input.bytesize < 10
          __gz_raise(GzipFile::Error, "not in gzip format", @gz_input.empty? ? nil : @gz_input)
        end
        __gz_raise(GzipFile::Error, "unexpected end of file")
      end
    end

    # Parse the member's header off the front of the input. Answers false
    # while the bytes so far are only a valid PREFIX; a wrong byte raises as
    # soon as it arrives.
    def __try_parse_header
      input = @gz_input
      __gz_raise(GzipFile::Error, "not in gzip format", input) if input.bytesize >= 1 && input.getbyte(0) != 0x1f
      __gz_raise(GzipFile::Error, "not in gzip format", input) if input.bytesize >= 2 && input.getbyte(1) != 0x8b
      return false if input.bytesize < 10
      __gz_raise(GzipFile::Error, "unsupported compression method", input) if input.getbyte(2) != 0x08
      flg = input.getbyte(3)
      at = 10
      if flg & 0x04 != 0
        return false if input.bytesize < at + 2
        at += 2 + input.byteslice(at, 2).unpack1("v")
        return false if input.bytesize < at
      end
      orig_name = nil
      if flg & 0x08 != 0
        nul = input.index("\0".b, at)
        return false unless nul
        orig_name = input.byteslice(at, nul - at)
        at = nul + 1
      end
      comment = nil
      if flg & 0x10 != 0
        nul = input.index("\0".b, at)
        return false unless nul
        comment = input.byteslice(at, nul - at)
        at = nul + 1
      end
      if flg & 0x02 != 0
        return false if input.bytesize < at + 2
        at += 2
      end
      @gz_mtime = input.byteslice(4, 4).unpack1("V")
      @gz_xfl = input.getbyte(8)
      @gz_os = input.getbyte(9)
      @gz_orig_name = orig_name
      @gz_comment = comment
      @gz_input = input.byteslice(at..) || +"".b
      true
    end

    def __char_len(lead)
      case lead
      when 0x00..0x7f then 1
      when 0xc2..0xdf then 2
      when 0xe0..0xef then 3
      when 0xf0..0xf4 then 4
      else 1
      end
    end

    def __line_args(args)
      sep = "\n"
      limit = nil
      case args.size
      when 0
        # the defaults
      when 1
        a = args[0]
        if a.nil?
          sep = nil
        elsif a.respond_to?(:to_str)
          sep = a.to_str
        else
          begin
            limit = a.to_int
          rescue NoMethodError
            raise TypeError, "no implicit conversion of #{a.class} into Integer"
          end
        end
      when 2
        sep = args[0].nil? ? nil : args[0].to_str
        limit = args[1].nil? ? nil : args[1].to_int
      else
        raise ArgumentError, "wrong number of arguments (given #{args.size}, expected 0..2)"
      end
      [sep, limit]
    end

    # One line, in the external encoding. A nil separator means the rest of
    # the member; "" is paragraph mode; a limit cuts the line at that many
    # bytes without splitting a character.
    def __getline(sep, limit)
      paragraph = sep == ""
      search = paragraph ? "\n\n".b : sep && sep.b
      if paragraph
        loop do
          __fill(1)
          break if @gz_buf.empty? || @gz_buf.getbyte(0) != 0x0a
          __take(1)
        end
      end
      searched = 0
      line = nil
      loop do
        if search && !search.empty?
          hit = @gz_buf.index(search, [searched, @gz_buf.bytesize].min)
          if hit && (limit.nil? || hit + search.bytesize <= limit)
            line = __take(hit + search.bytesize)
            break
          end
          searched = [@gz_buf.bytesize - (search.bytesize - 1), 0].max
        end
        if limit && @gz_buf.bytesize >= limit
          line = __take(__char_ceil(limit))
          break
        end
        break if @stream_end
        __decode_more
      end
      if line.nil?
        return nil if __check_when_drained
        line = __take(@gz_buf.bytesize)
      end
      @gz_lineno += 1
      line.force_encoding(__external)
    end

    # Extend a byte count to the end of the character it lands inside, so a
    # limit never splits one -- IO's own rule.
    def __char_ceil(limit)
      at = limit
      at += 1 while at < @gz_buf.bytesize && (@gz_buf.getbyte(at) & 0xc0) == 0x80
      at
    end

    # The caller's buffer keeps its own encoding; only the bytes change.
    def __into(outbuf, data)
      return data if outbuf.nil?
      enc = outbuf.encoding
      outbuf.replace(data)
      outbuf.force_encoding(enc)
    end
  end
end

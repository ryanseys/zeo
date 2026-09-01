# frozen_string_literal: true

# A pure-Ruby StringIO over a SHARED String buffer -- the caller's own
# object, never a copy, so mutation on either side is visible to the other.
#
# Positions are BYTES. Buffer, position, line counter and the stream's own
# encoding live in one carrier object that `dup`/`clone` and
# `reopen(a_stringio)` SHARE -- reading off a dup advances the original,
# ruby's own arrangement -- while open/closed state stays per-object, so
# closing one twin leaves the other open. Every buffer edit goes through one
# splice helper that works on raw bytes regardless of the buffer's encoding.
# `StringIO.new(nil)` is the null device: reads answer nil, writes are
# dropped, the string stays nil.
class StringIO
  include Enumerable

  VERSION = '3.2.0'

  MAX_LENGTH = 2**63 - 1

  ReadTimeout = Class.new(IOError) unless defined?(ReadTimeout)

  # The state `dup` twins share: the buffer, the cursor into it, and the
  # stream's own encoding (which survives a frozen buffer that cannot be
  # retagged).
  class Carrier # :nodoc:
    attr_accessor :string, :pos, :lineno, :encoding
  end
  private_constant :Carrier

  def self.open(*args, **opts)
    io = new(*args, **opts)
    return io unless block_given?

    begin
      yield io
    ensure
      io.send(:__close_quietly)
    end
  end

  def initialize(string = +'', mode = nil, **opts)
    if block_given?
      warn 'StringIO::new() does not take block; use StringIO::open() instead',
           uplevel: 1
    end
    mode = opts[:mode] if mode.nil? && opts.key?(:mode)
    unless string.nil?
      string = String.try_convert(string) || raise(TypeError,
        "can't convert #{string.class} into String")
    end
    @carrier = Carrier.new
    @carrier.string = string
    @carrier.pos = 0
    @carrier.lineno = 0
    @closed_read = false
    @closed_write = false
    frozen_source = !string.nil? && string.frozen?
    enc_spec = nil
    if mode.nil?
      @readable = true
      @writable = !frozen_source
      @append = false
    else
      @readable, @writable, @append, truncate_now, enc_spec = __parse_mode(mode)
      raise Errno::EACCES if @writable && frozen_source
      if string && (truncate_now || (@writable && !@readable && !@append))
        __splice(0, string.bytesize, '')
      end
    end
    __apply_open_encoding(enc_spec) if enc_spec
    self
  end

  # The twins share the carrier -- position and buffer -- but not
  # closedness: closing one leaves the other open.
  def initialize_copy(orig)
    @carrier = orig.instance_variable_get(:@carrier)
    self
  end

  def string = @carrier.string
  def lineno = @carrier.lineno

  def lineno=(value)
    @carrier.lineno = value
  end

  def string=(other)
    __check_self_frozen
    @carrier.string = other
    @carrier.pos = 0
    @carrier.lineno = 0
    @carrier.encoding = nil
    other
  end

  def pos = @carrier.pos
  alias_method :tell, :pos

  def pos=(new_pos)
    raise Errno::EINVAL if new_pos < 0
    @carrier.pos = new_pos
  end

  def seek(offset, whence = IO::SEEK_SET)
    __check_open
    base =
      case whence
      when IO::SEEK_SET then 0
      when IO::SEEK_CUR then @carrier.pos
      when IO::SEEK_END then __bytesize
      else raise Errno::EINVAL
      end
    target = base + offset
    raise Errno::EINVAL if target < 0
    @carrier.pos = target
    0
  end

  # Unlike seek/pos=, rewind also resets the line counter.
  def rewind
    @carrier.pos = 0
    @carrier.lineno = 0
    0
  end

  def size = __bytesize
  alias_method :length, :size

  def eof? = @carrier.pos >= __bytesize
  alias_method :eof, :eof?

  def truncate(len)
    __check_writable
    raise Errno::EINVAL, 'negative length' if len < 0
    return 0 if __null?

    if len < __bytesize
      __splice(len, __bytesize - len, '')
    else
      __splice(__bytesize, 0, "\0" * (len - __bytesize))
    end
    0
  end

  # Reading

  def read(length = nil, buffer = nil)
    __check_readable
    if length
      length = length.to_int
      raise ArgumentError, "negative length #{length} given" if length < 0
    end
    if __null?
      return __fill_buffer(buffer, ''.b, keep_encoding: true) if length == 0
      return __fill_buffer(buffer, nil, keep_encoding: !length.nil?)
    end

    if length.nil?
      slice = @carrier.string.byteslice(@carrier.pos, [__bytesize - @carrier.pos, 0].max) || ''
      slice = slice.dup.force_encoding(__encoding)
      @carrier.pos += slice.bytesize
      __fill_buffer(buffer, slice, keep_encoding: false)
    else
      return __fill_buffer(buffer, nil, keep_encoding: true) if @carrier.pos >= __bytesize && length > 0
      slice = @carrier.string.byteslice(@carrier.pos, length).to_s.b
      @carrier.pos += slice.bytesize
      __fill_buffer(buffer, slice, keep_encoding: true)
    end
  end

  # With no length, `sysread`/`readpartial` read the rest in the stream's
  # own encoding and answer "" at the end; a length means bytes, and the
  # end raises.
  def readpartial(maxlen = nil, buffer = nil)
    __check_readable
    return read(nil, buffer) if maxlen.nil?

    maxlen = maxlen.to_int
    raise ArgumentError, "negative length #{maxlen} given" if maxlen < 0
    raise EOFError, 'end of file reached' if __null? || @carrier.pos >= __bytesize

    slice = @carrier.string.byteslice(@carrier.pos, maxlen).to_s.b
    @carrier.pos += slice.bytesize
    __fill_buffer(buffer, slice, keep_encoding: true)
  end
  alias_method :sysread, :readpartial

  def read_nonblock(maxlen, buffer = nil, exception: true)
    __check_readable
    maxlen = maxlen.to_int
    raise ArgumentError, "negative length #{maxlen} given" if maxlen < 0
    if __null? || @carrier.pos >= __bytesize
      raise EOFError, 'end of file reached' if exception
      return nil
    end
    readpartial(maxlen, buffer)
  end

  def pread(maxlen, offset, buffer = nil)
    __check_readable
    maxlen = maxlen.to_int
    raise ArgumentError, "negative length #{maxlen} given" if maxlen < 0
    # A zero-length pread answers before looking at anything else -- the
    # out-buffer is NOT touched.
    return buffer || ''.b if maxlen == 0

    offset = offset.to_int
    raise Errno::EINVAL if offset < 0
    raise EOFError, 'end of file reached' if __null? || offset >= __bytesize

    __fill_buffer(buffer, @carrier.string.byteslice(offset, maxlen).to_s.b, keep_encoding: true)
  end

  def getc
    __check_readable
    return nil if __null? || eof?

    ch = __char_at(@carrier.pos)
    @carrier.pos += ch.bytesize
    ch
  end

  def readchar
    getc || raise(EOFError, 'end of file reached')
  end

  def getbyte
    __check_readable
    return nil if __null?

    b = @carrier.string.getbyte(@carrier.pos)
    @carrier.pos += 1 if b
    b
  end

  def readbyte
    getbyte || raise(EOFError, 'end of file reached')
  end

  # Push bytes back: back over what position allows and overwrite it;
  # whatever cannot be backed over is INSERTED, which is what makes an
  # unget at position 0 a prepend rather than an overwrite of the head.
  def ungetc(arg)
    __check_readable
    __check_unget_target
    return nil if arg.nil? || __null?

    arg = arg.chr(__encoding) if arg.is_a?(Integer)
    __unget(arg.to_str)
  end

  def ungetbyte(arg)
    __check_readable
    __check_unget_target
    return nil if arg.nil? || __null?

    arg = (arg & 0xFF).chr if arg.is_a?(Integer)
    __unget(arg.to_str)
  end

  def gets(sep = $/, limit = nil, chomp: false)
    __check_readable
    line = __getline(sep, limit, chomp)
    @carrier.lineno += 1 if line
    line
  end

  def readline(*args, **opts)
    gets(*args, **opts) || raise(EOFError, 'end of file reached')
  end

  def readlines(sep = $/, limit = nil, chomp: false)
    if sep.is_a?(Integer) && limit.nil?
      limit = sep
      sep = $/
    end
    raise ArgumentError, 'invalid limit: 0 for readlines' if limit == 0

    lines = []
    while (line = gets(sep, limit, chomp: chomp))
      lines << line
    end
    lines
  end

  def each_line(sep = $/, limit = nil, chomp: false)
    unless block_given?
      return to_enum(:each_line, sep, limit, chomp: chomp)
    end
    if sep.is_a?(Integer) && limit.nil?
      limit = sep
      sep = $/
    end
    raise ArgumentError, 'invalid limit: 0 for each_line' if limit == 0

    while (line = gets(sep, limit, chomp: chomp))
      yield line
    end
    self
  end
  alias_method :each, :each_line

  def each_char
    return to_enum(:each_char) unless block_given?

    while (ch = getc)
      yield ch
    end
    self
  end

  def each_byte
    return to_enum(:each_byte) unless block_given?

    while (b = getbyte)
      yield b
    end
    self
  end

  def each_codepoint
    return to_enum(:each_codepoint) unless block_given?

    while (ch = getc)
      yield ch.ord
    end
    self
  end

  # Writing

  def write(*args)
    __check_writable
    args.sum do |arg|
      data = arg.is_a?(String) ? arg : arg.to_s
      __write(data)
      data.bytesize
    end
  end

  def syswrite(arg)
    write(arg)
  end

  def write_nonblock(arg, exception: true)
    write(arg)
  end

  def <<(arg)
    write(arg)
    self
  end

  def print(*args)
    args = [$_] if args.empty?
    args.each { |a| write(a) }
    write($\) if $\
    nil
  end

  def printf(fmt, *args)
    write(format(fmt, *args))
    nil
  end

  def puts(*args)
    if args.empty?
      write("\n")
    else
      args.each do |arg|
        if arg.is_a?(Array)
          puts(*arg)
        else
          line = arg.to_s
          write(line)
          write("\n") unless line.end_with?("\n")
        end
      end
    end
    nil
  end

  def putc(arg)
    __check_writable
    if arg.is_a?(String)
      __write(arg.byteslice(0, __char_len(arg)))
    else
      __write((arg.to_int & 0xFF).chr)
    end
    arg
  end

  # Closing

  def close
    @closed_read = true
    @closed_write = true
    nil
  end

  def close_read
    raise IOError, 'closing non-duplex IO for reading' unless @readable
    @closed_read = true
    nil
  end

  def close_write
    raise IOError, 'closing non-duplex IO for writing' unless @writable
    @closed_write = true
    nil
  end

  def closed? = @closed_read && @closed_write
  def closed_read? = @closed_read
  def closed_write? = @closed_write

  # IO-flavored odds and ends: no descriptor, no process, always synced.

  def fileno = nil
  def pid = nil
  def isatty = false
  alias_method :tty?, :isatty
  def flush = self
  def fsync = 0
  def sync = true

  def sync=(value)
    value
  end

  def binmode
    set_encoding(Encoding::BINARY)
    self
  end

  def external_encoding = __encoding
  def internal_encoding = nil

  def set_encoding(enc, _int_enc = nil, **_opts)
    unless enc.is_a?(Encoding)
      enc = enc.nil? ? 'ASCII-8BIT' : enc.to_s
      # An IO-style "external:internal" spec names the external half first.
      enc = Encoding.find(enc.split(':').first)
    end
    @carrier.encoding = enc
    s = @carrier.string
    if s && !s.frozen?
      # Retagging is not a MUTATION in ruby's own StringIO -- a chilled
      # literal must not hear its will-be-frozen warning here.
      deprecated = Warning[:deprecated]
      begin
        Warning[:deprecated] = false
        s.force_encoding(enc)
      ensure
        Warning[:deprecated] = deprecated
      end
    end
    self
  end

  def set_encoding_by_bom
    return nil if __null?

    bytes = @carrier.string.byteslice(0, 4).to_s.bytes
    enc, len =
      if bytes[0, 3] == [0xEF, 0xBB, 0xBF] then [Encoding::UTF_8, 3]
      elsif bytes[0, 4] == [0xFF, 0xFE, 0x00, 0x00] then [Encoding::UTF_32LE, 4]
      elsif bytes[0, 4] == [0x00, 0x00, 0xFE, 0xFF] then [Encoding::UTF_32BE, 4]
      elsif bytes[0, 2] == [0xFF, 0xFE] then [Encoding::UTF_16LE, 2]
      elsif bytes[0, 2] == [0xFE, 0xFF] then [Encoding::UTF_16BE, 2]
      end
    return nil unless enc

    set_encoding(enc)
    @carrier.pos = len
    enc
  end

  # `reopen(a_stringio)` SHARES its stream -- carrier and all, like `dup` --
  # while `reopen(a_string, mode)` restarts this one over a new buffer.
  def reopen(other = nil, mode = nil)
    __check_self_frozen
    if other.is_a?(StringIO)
      @carrier = other.instance_variable_get(:@carrier)
      @readable = other.instance_variable_get(:@readable)
      @writable = other.instance_variable_get(:@writable)
      @append = other.instance_variable_get(:@append)
    elsif other.nil? && mode.nil?
      initialize(+'', 'w+')
    else
      initialize(other, mode)
    end
    self
  end

  def fcntl(*)
    raise NotImplementedError, 'fcntl() function is unimplemented on this machine'
  end

  # A StringIO owns a position into a live buffer; neither survives a
  # round trip, so ruby refuses to dump one.
  def marshal_dump
    raise TypeError, "no _dump_data is defined for class #{self.class}"
  end

  private

  def __null? = @carrier.string.nil?

  def __bytesize
    s = @carrier.string
    s ? s.bytesize : 0
  end

  def __encoding
    @carrier.encoding || begin
      s = @carrier.string
      s ? s.encoding : Encoding::UTF_8
    end
  end

  # Mode forms: a string ("r+", "wb:utf-32be", "rb:BOM|UTF-8"), or IO's
  # integer flags. Answers [readable, writable, append, truncate, enc_spec].
  def __parse_mode(mode)
    if mode.is_a?(Integer)
      acc = mode & 3
      return [
        acc != File::WRONLY,
        acc != File::RDONLY,
        mode.anybits?(File::APPEND),
        mode.anybits?(File::TRUNC),
        nil,
      ]
    end
    mode = mode.to_str unless mode.is_a?(String)
    head, enc_spec = mode.split(':', 2)
    unless head.match?(/\A[rwa](?:b\+?|t\+?|\+[bt]?)?\z/)
      raise ArgumentError, "invalid access mode #{mode}"
    end
    plus = head.include?('+')
    dir =
      case head[0]
      when 'r' then [true, plus, false, false]
      when 'w' then [plus, true, false, true]
      else [plus, true, true, false]
      end
    dir + [enc_spec]
  end

  # The `:enc` tail of an open mode. `BOM|<enc>` consumes a byte-order
  # mark when one is present and takes its encoding, falling back to the
  # named one; a plain name just tags the stream. A writable empty stream
  # tagged this way CONVERTS what lands in it (see `__write`).
  def __apply_open_encoding(spec)
    if spec.start_with?('BOM|')
      set_encoding_by_bom || set_encoding(spec.delete_prefix('BOM|'))
    else
      set_encoding(spec)
    end
  end

  # `IO#read`'s buffer contract: with an out-buffer, the answer lands in it
  # (replacing its content, keeping its identity). A byte-wise read keeps
  # the buffer's OWN encoding; a whole-stream read imposes the stream's.
  # A nil slice -- read past the end -- empties the buffer and answers nil.
  def __fill_buffer(buffer, slice, keep_encoding:)
    return slice if buffer.nil?

    enc = buffer.encoding
    buffer.replace(slice || '')
    buffer.force_encoding(enc) if keep_encoding
    slice.nil? ? nil : buffer
  end

  def __check_readable
    raise IOError, 'uninitialized stream' unless defined?(@carrier)
    raise IOError, 'not opened for reading' if @closed_read || !@readable
  end

  def __check_writable
    __check_self_frozen
    raise IOError, 'uninitialized stream' unless defined?(@carrier)
    raise IOError, 'not opened for writing' if @closed_write || !@writable
    if !__null? && @carrier.string.frozen?
      raise IOError, 'not modifiable string'
    end
  end

  # ungetc/ungetbyte are READ-side calls that still MUTATE the buffer, so
  # a frozen buffer refuses them the way a write would.
  def __check_unget_target
    if !__null? && @carrier.string.frozen?
      raise IOError, 'not modifiable string'
    end
  end

  def __check_open
    raise IOError, 'closed stream' if closed?
  end

  def __check_self_frozen
    raise FrozenError, "can't modify frozen #{self.class}: #{inspect}" if frozen?
  end

  # `open`'s ensure: close whatever directions this mode has, silently.
  def __close_quietly
    @closed_read = true
    @closed_write = true
  end

  # The one buffer editor: replace `remove` bytes at `at` with `data`,
  # BYTE-wise regardless of what the buffer's encoding thinks of either
  # side. The encoding flip is what lets binary data land in a text buffer,
  # exactly as the C extension's memmove does.
  def __splice(at, remove, data)
    s = @carrier.string
    enc = s.encoding
    s.force_encoding(Encoding::BINARY)
    begin
      s.bytesplice(at, remove, data.b)
    ensure
      s.force_encoding(enc)
    end
  end

  def __write(data)
    return if __null?

    # IO's encoding rule, in order: binary data (or a binary stream) lands
    # byte-wise; a US-ASCII stream never converts -- foreign data UPGRADES
    # an all-ascii buffer to its own encoding instead; any other stream
    # CONVERTS what lands in it, and what cannot convert is ruby's
    # CompatibilityError. A non-ascii-compatible stream (UTF-32) converts
    # even ascii-only data, or its bytes would be garbage.
    s = @carrier.string
    enc = __encoding
    if data.encoding != enc && data.encoding != Encoding::BINARY &&
       enc != Encoding::BINARY && !data.ascii_only?
      if enc == Encoding::US_ASCII
        if s.ascii_only? && data.encoding.ascii_compatible?
          s.force_encoding(data.encoding)
          @carrier.encoding = nil
        end
      else
        begin
          data = data.encode(enc)
        rescue Encoding::UndefinedConversionError, Encoding::InvalidByteSequenceError,
               Encoding::ConverterNotFoundError
          raise Encoding::CompatibilityError,
            "incompatible character encodings: #{data.encoding} and #{enc}"
        end
      end
    elsif !enc.ascii_compatible? && data.encoding != enc
      data = data.encode(enc)
    end
    at = @append ? __bytesize : @carrier.pos
    if at + data.bytesize > MAX_LENGTH
      raise ArgumentError, 'string size too big'
    end
    if at > __bytesize
      __splice(__bytesize, 0, "\0" * (at - __bytesize))
    end
    __splice(at, [data.bytesize, __bytesize - at].min, data)
    @carrier.pos = at + data.bytesize
  end

  def __unget(data)
    back = [data.bytesize, @carrier.pos].min
    @carrier.pos -= back
    at = @carrier.pos
    if at > __bytesize
      __splice(__bytesize, 0, "\0" * (at - __bytesize))
    end
    __splice(at, [back, __bytesize - at].min, data)
    nil
  end

  # The shared line reader behind gets/readline/each_line. `sep` may be a
  # String (through to_str), nil (slurp) or empty (paragraph mode); a lone
  # Integer argument is the limit.
  def __getline(sep, limit, chomp)
    if sep.is_a?(Integer) && limit.nil?
      limit = sep
      sep = $/
    end
    unless sep.nil? || sep.is_a?(String)
      sep = String.try_convert(sep) || raise(TypeError,
        "no implicit conversion of #{sep.class} into String")
    end
    limit = limit.to_int unless limit.nil?
    return nil if __null?

    pos = @carrier.pos
    total = __bytesize

    if sep && sep.empty?
      # Paragraph mode: skip the newlines the last paragraph left behind.
      pos += 1 while pos < total && @carrier.string.getbyte(pos) == 10
      @carrier.pos = pos
    end
    return nil if pos >= total
    return (+'').force_encoding(__encoding) if limit == 0

    raw = @carrier.string.b
    sep_end = nil
    stop =
      if sep.nil?
        total
      elsif sep.empty?
        # A paragraph ends at a run of two or more newlines, each `\r?\n`.
        if (m = raw.match(/(?:\r?\n){2,}/n, pos))
          sep_end = m.begin(0)
          m.end(0)
        else
          total
        end
      else
        sep_b = __in_buffer_encoding(sep).b
        if (i = raw.index(sep_b, pos))
          i + sep_b.bytesize
        else
          total
        end
      end
    if limit && pos + limit < stop
      stop = __char_ceil(pos + limit)
      sep_end = nil
    end
    line = @carrier.string.byteslice(pos, stop - pos).force_encoding(__encoding)
    @carrier.pos = stop
    if chomp && sep
      line =
        if sep.empty?
          if sep_end
            @carrier.string.byteslice(pos, sep_end - pos).force_encoding(__encoding)
          else
            line
          end
        elsif sep == "\n"
          # The default separator's chomp also takes a \r\n line ending.
          line.sub(/\r?\n\z/, '')
        elsif line.end_with?(__in_buffer_encoding(sep))
          line.byteslice(0, line.bytesize - __in_buffer_encoding(sep).bytesize)
              .force_encoding(__encoding)
        else
          line
        end
    end
    line
  end

  # The separator, spelled in the stream's own encoding, so the byte search
  # finds it inside a wide encoding too.
  def __in_buffer_encoding(sep)
    return sep if sep.encoding == __encoding || !sep.ascii_only?

    sep.encode(__encoding)
  rescue Encoding::ConverterNotFoundError
    sep
  end

  # The smallest character-boundary byte offset at or after `at`: a limit
  # never splits a character.
  def __char_ceil(at)
    s = @carrier.string
    return at if at >= s.bytesize || !__mid_char?(at)

    # Walk back to the char `at` splits, then to its end.
    probe = at
    probe -= 1 while probe > 0 && __mid_char?(probe)
    probe + __char_at(probe).bytesize
  end

  def __mid_char?(at)
    head = @carrier.string.byteslice(0, at)
    !head.valid_encoding? && at > 0
  end

  # One whole character at byte `at`, in the stream's own encoding.
  def __char_at(at)
    s = @carrier.string
    tail = s.byteslice(at, s.bytesize - at).force_encoding(__encoding)
    ch = tail[0]
    # A broken byte that decodes to nothing is handed back alone.
    ch.nil? || ch.empty? ? tail.byteslice(0, 1).force_encoding(__encoding) : ch
  end

  def __char_len(str)
    first = str[0]
    first.nil? || first.empty? ? 1 : first.bytesize
  end
end

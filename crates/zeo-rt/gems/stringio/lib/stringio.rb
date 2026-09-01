# frozen_string_literal: true

# A pure-Ruby StringIO over a SHARED String buffer -- the caller's own
# object, never a copy, so mutation on either side is visible to the other,
# exactly as in the C extension.
#
# Positions are BYTES. Every buffer edit goes through one splice helper that
# works on raw bytes regardless of the buffer's encoding; every read hands
# back a byteslice, which carries the buffer's encoding by itself. The
# surface is held to the same committed golden the native extension answers,
# with ruby's C gem as the recorded text.
class StringIO
  include Enumerable

  VERSION = '3.2.0'

  ReadTimeout = Class.new(IOError) unless defined?(ReadTimeout)

  def self.open(*args)
    io = new(*args)
    return io unless block_given?

    begin
      yield io
    ensure
      io.send(:__close_quietly)
    end
  end

  def initialize(string = nil, mode = nil)
    string = +'' if string.nil?
    string = String.try_convert(string) || raise(TypeError,
      "no implicit conversion of #{string.class} into String")
    @string = string
    frozen_source = @string.frozen?
    if mode.nil?
      @readable = true
      @writable = !frozen_source
      @append = false
    else
      mode = mode.to_str unless mode.is_a?(String)
      head = mode.split(':').first || mode
      plus = head.include?('+')
      @readable, @writable, @append =
        case head[0]
        when 'r' then [true, plus, false]
        when 'w' then [plus, true, false]
        when 'a' then [plus, true, true]
        else raise ArgumentError, "invalid access mode #{mode}"
        end
      raise Errno::EACCES if @writable && frozen_source
    end
    __splice(0, @string.bytesize, '') if @writable && !@readable && !@append
    @pos = 0
    @lineno = 0
    @closed_read = false
    @closed_write = false
    self
  end

  attr_reader :string, :lineno
  attr_writer :lineno

  def string=(other)
    @string = other
    @pos = 0
    @lineno = 0
    other
  end

  def pos = @pos
  alias_method :tell, :pos

  def pos=(new_pos)
    raise Errno::EINVAL if new_pos < 0
    @pos = new_pos
  end

  def seek(offset, whence = IO::SEEK_SET)
    base =
      case whence
      when IO::SEEK_SET then 0
      when IO::SEEK_CUR then @pos
      when IO::SEEK_END then @string.bytesize
      else raise Errno::EINVAL
      end
    target = base + offset
    raise Errno::EINVAL if target < 0
    @pos = target
    0
  end

  # Unlike seek/pos=, rewind also resets the line counter.
  def rewind
    @pos = 0
    @lineno = 0
    0
  end

  def size = @string.bytesize
  alias_method :length, :size

  def eof? = @pos >= @string.bytesize
  alias_method :eof, :eof?

  def truncate(len)
    __check_writable
    raise Errno::EINVAL, 'negative length' if len < 0

    if len < @string.bytesize
      __splice(len, @string.bytesize - len, '')
    else
      __splice(@string.bytesize, 0, "\0" * (len - @string.bytesize))
    end
    0
  end

  # Reading

  def read(length = nil, buffer = nil)
    __check_readable
    slice =
      if length.nil?
        @string.byteslice(@pos, [@string.bytesize - @pos, 0].max) || ''
      else
        raise ArgumentError, "negative length #{length} given" if length < 0
        return __fill_buffer(buffer, nil) if @pos >= @string.bytesize && length > 0
        @string.byteslice(@pos, length).to_s.b
      end
    @pos += slice.bytesize
    __fill_buffer(buffer, slice)
  end

  def readpartial(maxlen, buffer = nil)
    __check_readable
    raise EOFError, 'end of file reached' if @pos >= @string.bytesize

    slice = @string.byteslice(@pos, maxlen).to_s.b
    @pos += slice.bytesize
    __fill_buffer(buffer, slice)
  end
  alias_method :sysread, :readpartial

  def read_nonblock(maxlen, buffer = nil, exception: true)
    __check_readable
    if @pos >= @string.bytesize
      raise EOFError, 'end of file reached' if exception
      return nil
    end
    readpartial(maxlen, buffer)
  end

  def pread(maxlen, offset, buffer = nil)
    __check_readable
    raise Errno::EINVAL if offset < 0
    raise EOFError, 'end of file reached' if offset >= @string.bytesize

    __fill_buffer(buffer, @string.byteslice(offset, maxlen).to_s.b)
  end

  def getc
    __check_readable
    return nil if eof?

    ch = __char_at(@pos)
    @pos += ch.bytesize
    ch
  end

  def readchar
    getc || raise(EOFError, 'end of file reached')
  end

  def getbyte
    __check_readable
    b = @string.getbyte(@pos)
    @pos += 1 if b
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
    return nil if arg.nil?

    arg = arg.chr(__encoding) if arg.is_a?(Integer)
    __unget(arg.to_str)
  end

  def ungetbyte(arg)
    __check_readable
    return nil if arg.nil?

    arg = (arg & 0xFF).chr if arg.is_a?(Integer)
    __unget(arg.to_str)
  end

  def gets(sep = $/, limit = nil, chomp: false)
    __check_readable
    if sep.is_a?(Integer) && limit.nil?
      limit = sep
      sep = $/
    end
    sep = sep.to_str unless sep.nil? || sep.is_a?(String)

    # An empty separator is PARAGRAPH mode: skip the newlines the last
    # paragraph left behind, then read through the next blank line and
    # every newline that follows it.
    if sep && sep.empty?
      @pos += 1 while @pos < @string.bytesize && @string.getbyte(@pos) == 10
    end
    return nil if @pos >= @string.bytesize

    stop =
      if sep.nil?
        @string.bytesize
      elsif sep.empty?
        if (i = __byteindex("\n\n", @pos))
          e = i + 2
          e += 1 while e < @string.bytesize && @string.getbyte(e) == 10
          e
        else
          @string.bytesize
        end
      elsif (i = __byteindex(sep, @pos))
        i + sep.bytesize
      else
        @string.bytesize
      end
    stop = [stop, @pos + limit].min if limit
    line = @string.byteslice(@pos, stop - @pos)
    @pos = stop
    @lineno += 1
    line = line.sub(/\n\z/, '') if chomp
    line
  end

  def readline(...)
    gets(...) || raise(EOFError, 'end of file reached')
  end

  def readlines(...)
    lines = []
    while (line = gets(...))
      lines << line
    end
    lines
  end

  def each_line(...)
    return to_enum(:each_line, ...) unless block_given?

    while (line = gets(...))
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
    enc = Encoding.find(enc || 'ASCII-8BIT') unless enc.is_a?(Encoding)
    @string.force_encoding(enc) unless @string.frozen?
    self
  end

  def set_encoding_by_bom
    bytes = @string.byteslice(0, 4).to_s.bytes
    enc, len =
      if bytes[0, 3] == [0xEF, 0xBB, 0xBF] then [Encoding::UTF_8, 3]
      elsif bytes[0, 4] == [0xFF, 0xFE, 0x00, 0x00] then [Encoding::UTF_32LE, 4]
      elsif bytes[0, 4] == [0x00, 0x00, 0xFE, 0xFF] then [Encoding::UTF_32BE, 4]
      elsif bytes[0, 2] == [0xFF, 0xFE] then [Encoding::UTF_16LE, 2]
      elsif bytes[0, 2] == [0xFE, 0xFF] then [Encoding::UTF_16BE, 2]
      end
    return nil unless enc

    set_encoding(enc)
    @pos = len
    enc
  end

  def reopen(other = nil, mode = nil)
    if other.is_a?(StringIO)
      initialize(other.string, mode)
    else
      initialize(other, mode || 'w+')
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

  def __encoding = @string.encoding

  # `IO#read`'s buffer contract: with an out-buffer, the answer lands in it
  # (replacing its content, keeping its identity); a nil slice -- read past
  # the end -- empties the buffer and answers nil.
  def __fill_buffer(buffer, slice)
    return slice if buffer.nil?

    buffer.replace(slice || '')
    slice.nil? ? nil : buffer
  end

  def __check_readable
    raise IOError, 'not opened for reading' if @closed_read || !@readable
  end

  def __check_writable
    raise IOError, 'not opened for writing' if @closed_write || !@writable
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
    enc = @string.encoding
    @string.force_encoding(Encoding::BINARY)
    begin
      @string.bytesplice(at, remove, data.b)
    ensure
      @string.force_encoding(enc)
    end
  end

  def __write(data)
    at = @append ? @string.bytesize : @pos
    if at > @string.bytesize
      __splice(@string.bytesize, 0, "\0" * (at - @string.bytesize))
    end
    __splice(at, [data.bytesize, @string.bytesize - at].min, data)
    @pos = at + data.bytesize
  end

  def __unget(data)
    back = [data.bytesize, @pos].min
    @pos -= back
    if @pos > @string.bytesize
      __splice(@string.bytesize, 0, "\0" * (@pos - @string.bytesize))
    end
    __splice(@pos, [back, @string.bytesize - @pos].min, data)
    nil
  end

  def __byteindex(needle, from)
    @string.b.index(needle.b, from)
  end

  # One whole character at byte `at`, in the buffer's own encoding.
  def __char_at(at)
    tail = @string.byteslice(at, @string.bytesize - at)
    ch = tail[0]
    # A broken byte that decodes to nothing is handed back alone.
    ch.nil? || ch.empty? ? tail.byteslice(0, 1) : ch
  end

  def __char_len(str)
    first = str[0]
    first.nil? || first.empty? ? 1 : first.bytesize
  end
end

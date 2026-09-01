# frozen_string_literal: true

# A pure-Ruby StringScanner, structured after strscan's own
# lib/strscan/truffleruby.rb (3.1.8) with that engine's primitives replaced
# by plain Ruby over this runtime's String and Regexp.
#
# Positions are BYTES, as in the C extension. In the default mode a pattern
# matches against the REST of the string as if it were fresh (`\A` means the
# scan position), so matching runs against a byte-sliced tail and every
# byte offset is rebased through @match_base. `fixed_anchor: true` matches
# against the whole string with the position as only the search start, so
# `\A` keeps meaning the string's own head. String patterns never touch the
# regexp engine: they compare and search BYTES, which is also what lets a
# broken-encoding subject scan byte by byte.
class StringScanner
  class Error < StandardError
  end
  # :stopdoc:
  unless ::Object.const_defined?(:ScanError)
    ::Object::ScanError = Error
    ::Object.deprecate_constant :ScanError
  end

  Version = '3.1.8'
  Id = '$Id$'

  def self.must_C_version = self
  # :startdoc:

  attr_reader :string, :pos
  alias_method :pointer, :pos

  def initialize(string, options = nil, fixed_anchor: false)
    @string = __to_str(string)
    @fixed_anchor = !!fixed_anchor
    @pos = 0
    @last_match = nil
    @literal_match = nil
    @match_base = 0
    @prev_pos = 0
  end

  def inspect
    return "#<#{self.class} (uninitialized)>" unless @string
    return "#<#{self.class} fin>" if eos?

    before =
      if @pos == 0
        ''
      elsif @pos <= 5
        "#{@string.byteslice(0, @pos).inspect} "
      else
        "#{('...' + @string.byteslice(@pos - 5, 5)).inspect} "
      end

    after =
      if @pos >= @string.bytesize - 5
        " #{@string.byteslice(@pos..).inspect}"
      else
        " #{(@string.byteslice(@pos, 5) + '...').inspect}"
      end

    "#<#{self.class} #{@pos}/#{@string.bytesize} #{before}@#{after}>"
  end

  def pos=(new_pos)
    if new_pos < 0
      new_pos += @string.bytesize
    end
    raise RangeError, 'index out of range' if new_pos < 0
    raise RangeError, 'index out of range' if new_pos > @string.bytesize
    @pos = new_pos
  end
  alias_method :pointer=, :pos=

  def charpos = @string.byteslice(0, @pos).length

  def rest = @string.byteslice(@pos, @string.bytesize) || ''

  def rest_size = @string.bytesize - @pos.clamp(0, @string.bytesize)

  def concat(more_string)
    @string.concat(__to_str(more_string))
    self
  end
  alias_method :<<, :concat

  def string=(other_string)
    @string = __to_str(other_string)
    @pos = 0
    __miss
    other_string
  end

  def reset
    @pos = 0
    __miss
    self
  end

  def terminate
    @pos = @string.bytesize
    __miss
    self
  end

  # Restores the position from BEFORE the last match operation -- not the
  # match's own beginning, which differs from it after an until-family hit.
  def unscan
    if matched?
      @pos = @prev_pos
      __miss
      self
    else
      raise Error, 'unscan failed: previous match record not exist'
    end
  end

  # Predicates

  def fixed_anchor? = @fixed_anchor

  def beginning_of_line?
    @pos == 0 or @string.byteslice(@pos - 1, 1) == "\n"
  end
  alias_method :bol?, :beginning_of_line?

  def eos?
    @pos >= @string.bytesize
  end

  def rest?
    !eos?
  end

  # MatchData-like methods

  def matched? = !!(@literal_match || @last_match)

  def matched
    range = __matched_range
    @string.byteslice(range[0], range[1] - range[0]) if range
  end

  # The C scanner's own indexing, not MatchData's: a negative index wraps
  # over ALL registers INCLUDING the whole match, so `s[-1]` after a
  # capture-free hit is the match itself (MatchData answers nil there).
  def [](group)
    raise TypeError, 'no implicit conversion of Range into Integer' if group.is_a?(Range)

    if @literal_match
      case group
      when Integer
        group == 0 || group == -1 ? matched : nil
      when Symbol, String
        raise IndexError, "undefined group name reference: #{group}"
      else
        nil
      end
    elsif @last_match
      if group.is_a?(Integer) && group < 0
        idx = @last_match.size + group
        idx < 0 ? nil : @last_match[idx]
      else
        @last_match[group]
      end
    end
  end

  def values_at(*groups)
    groups.map { |g| self[g] } if matched?
  end

  def captures
    if @literal_match
      []
    elsif @last_match
      @last_match.captures
    end
  end

  def size
    if @literal_match
      1
    else
      @last_match&.size
    end
  end

  # Answered from the scanner's recorded byte range against the FULL
  # string, never from the MatchData: the default mode matched a tail
  # slice, whose own pre_match would lose everything before the position.
  def pre_match
    @string.byteslice(0, __byte_begin) if matched?
  end

  def post_match
    @string.byteslice(__byte_end..) if matched?
  end

  def named_captures
    return {} if @literal_match
    @last_match&.named_captures || {}
  end

  def matched_size
    __byte_end - __byte_begin if matched?
  end

  # Scan-like methods

  def peek(length)
    raise ArgumentError, 'negative string size (or size too big)' if length < 0
    @string.byteslice(@pos, length) || ''
  end

  def peek_byte = @string.getbyte(@pos)

  def get_byte
    @prev_pos = @pos
    return __miss if eos?

    byte = @string.byteslice(@pos, 1)
    __record_literal(@pos, @pos + 1)
    @pos += 1
    byte
  end

  def scan_byte
    return nil if eos?

    byte_value = @string.getbyte(@pos)
    get_byte
    byte_value
  end

  def getch = scan(/./m)

  def scan_integer(base: 10)
    case base
    when 10
      scan(/[+-]?\d+/)&.to_i
    when 16
      scan(/[+-]?(0x)?[0-9a-fA-F]+/)&.to_i(16)
    else
      raise ArgumentError, "Unsupported integer base: #{base.inspect}, expected 10 or 16"
    end
  end

  def scan_full(pattern, advance_pointer, return_string)
    if advance_pointer
      if return_string
        scan(pattern)
      else
        skip(pattern)
      end
    else
      if return_string
        check(pattern)
      else
        match?(pattern)
      end
    end
  end

  def search_full(pattern, advance_pointer, return_string)
    if advance_pointer
      if return_string
        scan_until(pattern)
      else
        skip_until(pattern)
      end
    else
      if return_string
        check_until(pattern)
      else
        exist?(pattern)
      end
    end
  end

  # Keep the following 8 methods in sync, they are small variations of one
  # another

  # Matches at start methods

  # Matches at start, returns matched string, does not advance position
  def check(pattern)
    if __match_at_start(pattern)
      matched
    end
  end

  # Matches at start, returns matched string, advances position
  def scan(pattern)
    if __match_at_start(pattern)
      s = matched
      @pos = __byte_end
      s
    end
  end

  # Matches at start, returns matched bytesize, does not advance position
  def match?(pattern)
    if __match_at_start(pattern)
      __byte_end - @pos
    end
  end

  # Matches at start, returns matched bytesize, advances position
  def skip(pattern)
    prev = @pos
    if __match_at_start(pattern)
      @pos = __byte_end
      @pos - prev
    end
  end

  # Matches anywhere methods

  # Matches anywhere, returns matched string, does not advance position
  def check_until(pattern)
    prev = @pos
    if __search(pattern)
      @string.byteslice(prev, __byte_end - prev)
    end
  end

  # Matches anywhere, returns matched string, advances position
  def scan_until(pattern)
    prev = @pos
    if __search(pattern)
      @pos = __byte_end
      @string.byteslice(prev, @pos - prev)
    end
  end

  # Matches anywhere, returns matched bytesize, does not advance position
  def exist?(pattern)
    prev = @pos
    if __search(pattern)
      __byte_end - prev
    end
  end

  # Matches anywhere, returns matched bytesize, advances position
  def skip_until(pattern)
    prev = @pos
    if __search(pattern)
      @pos = __byte_end
      @pos - prev
    end
  end

  private

  def __to_str(obj)
    converted = String.try_convert(obj)
    unless converted
      raise TypeError,
            "no implicit conversion of #{obj.nil? ? 'nil' : obj.class} into String"
    end
    converted
  end

  # The absolute BYTE range of the last match: recorded directly for a
  # literal hit, rebased from the engine's own string (the tail in default
  # mode, the whole in fixed) for a regexp hit.
  def __matched_range
    if @literal_match
      @literal_match
    elsif @last_match
      b, e = @last_match.byteoffset(0)
      [@match_base + b, @match_base + e]
    end
  end

  def __byte_begin = __matched_range[0]
  def __byte_end = __matched_range[1]

  def __record(match)
    @last_match = match
    @literal_match = nil
    match
  end

  def __record_literal(from, to)
    @last_match = nil
    @literal_match = [from, to]
  end

  def __miss
    @last_match = nil
    @literal_match = nil
    nil
  end

  # `\A`-anchor the pattern once per Regexp; the wrap keeps the original's
  # options AND its source encoding (an /re/e stays EUC-JP-fixed), and the
  # cache keeps the wrap from recompiling every scan.
  ANCHORED = {}.compare_by_identity
  private_constant :ANCHORED

  def __anchored(pattern)
    ANCHORED[pattern] ||= begin
      src = pattern.source
      Regexp.new("\\A(?:#{src})".force_encoding(src.encoding), pattern.options)
    end
  end

  G_ANCHORED = {}.compare_by_identity
  private_constant :G_ANCHORED

  def __g_anchored(pattern)
    G_ANCHORED[pattern] ||= begin
      src = pattern.source
      Regexp.new("\\G(?:#{src})".force_encoding(src.encoding), pattern.options)
    end
  end

  # The C scanner raises through rb_enc_check before matching a string
  # pattern; byte comparison alone would silently accept what ruby refuses.
  def __check_encoding(lit)
    return if Encoding.compatible?(@string, lit)

    raise Encoding::CompatibilityError,
          "incompatible character encodings: #{@string.encoding.name} and #{lit.encoding.name}"
  end

  def __match_at_start(pattern)
    @prev_pos = @pos
    return __miss if @pos > @string.bytesize

    if pattern.is_a?(Regexp)
      if @fixed_anchor
        # `\G` anchors at the search start over the TRUE string, so `\A`
        # keeps meaning the string's own head -- this mode's definition.
        @match_base = 0
        __record(@string.match(__g_anchored(pattern), charpos))
      else
        tail = @string.byteslice(@pos, @string.bytesize)
        @match_base = @pos
        __record(tail.match(__anchored(pattern)))
      end
    else
      lit = __to_str(pattern)
      __check_encoding(lit)
      if @string.byteslice(@pos, lit.bytesize).b == lit.b
        __record_literal(@pos, @pos + lit.bytesize)
      else
        __miss
      end
    end
  end

  def __search(pattern)
    @prev_pos = @pos
    return __miss if @pos > @string.bytesize

    if pattern.is_a?(Regexp)
      if @fixed_anchor
        @match_base = 0
        __record(@string.match(pattern, charpos))
      else
        tail = @string.byteslice(@pos, @string.bytesize)
        @match_base = @pos
        __record(tail.match(pattern))
      end
    else
      lit = __to_str(pattern)
      __check_encoding(lit)
      at = @string.b.index(lit.b, @pos)
      if at
        __record_literal(at, at + lit.bytesize)
      else
        __miss
      end
    end
  end
end

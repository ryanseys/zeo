# A subset of Ruby's OptionParser, as a spin.toml package -- Ruby source the
# compiler splices like any other required file, since nothing here needs to
# reach below the language.
#
# WHAT IS HERE: `new` (with the configuring block), `banner`/`banner=`,
# `separator`, `on` (short and long switches, with or without a required
# argument, `--name=VALUE` and `--name VALUE` spellings, and bundled short
# flags like `-abc`), `parse!`/`parse`, `help`/`to_s`, and the
# `InvalidOption`/`MissingArgument` errors under `ParseError`.
#
# WHAT IS NOT: optional arguments (`--name [VALUE]`), `[no-]` negation, type
# coercion (`Integer`, `Array`, ...), `into:`, completion of abbreviated long
# options, `on_tail`/`on_head`, `default_argv`, and `load`/`environment`.
# Each of those is absent rather than stubbed, so reaching for one is a
# NoMethodError at the call rather than a silent no-op at parse time.

class OptionParser
  class ParseError < StandardError; end

  class InvalidOption < ParseError
    def initialize(arg)
      super("invalid option: #{arg}")
    end
  end

  class MissingArgument < ParseError
    def initialize(arg)
      super("missing argument: #{arg}")
    end
  end

  # One registered switch: how it is spelled, whether it consumes a value,
  # what to run when it fires, and how it prints in the help text.
  class Switch
    attr_reader :short, :long, :arg, :desc

    # `display` is the switch spellings exactly as the caller wrote them
    # ("-c, --count=N"), which is what the help text shows -- the parsed
    # `short`/`long`/`arg` are for MATCHING and can't be reassembled back
    # into the original spelling.
    def initialize(short, long, arg, desc, display, handler)
      @short = short
      @long = long
      @arg = arg
      @desc = desc
      @display = display
      @handler = handler
    end

    def takes_argument?
      !@arg.nil?
    end

    def call(value)
      @handler.call(value) if @handler
    end

    # The left column of the help text: a 4-space indent plus the spellings,
    # padded out to optparse's 32-wide summary column.
    def summary
      "    " + @display
    end
  end

  attr_accessor :banner

  def initialize
    @banner = nil
    @switches = []
    @summary = []
    yield self if block_given?
  end

  # A literal line in the help text, between switch groups.
  def separator(text)
    @summary.push(text)
    nil
  end

  # `on("-n", "--name NAME", "description") { |v| ... }`. Every argument is a
  # String: those starting with `-` are switch spellings, the rest is
  # description text. A spelling carrying a second word ("--name NAME") is
  # what makes the switch take a value.
  def on(*specs, &handler)
    short = nil
    long = nil
    arg = nil
    desc = []
    specs.each do |spec|
      if spec.start_with?("-")
        flag, value = split_spec(spec)
        arg = value if value
        if flag.start_with?("--")
          long = flag
        else
          short = flag
        end
      else
        desc.push(spec)
      end
    end
    display = []
    specs.each { |spec| display.push(spec) if spec.start_with?("-") }
    sw = Switch.new(short, long, arg, desc.join(" "), display.join(", "), handler)
    @switches.push(sw)
    @summary.push(sw)
    self
  end

  # Parse and REMOVE the recognized options from `argv`, leaving the
  # positional arguments behind. Returns `argv`.
  def parse!(argv)
    rest = []
    seen_terminator = false
    until argv.empty?
      arg = argv.shift
      if seen_terminator
        rest.push(arg)
      elsif arg == "--"
        # Everything after a bare `--` is positional, even if it looks like
        # a switch.
        seen_terminator = true
      elsif arg.start_with?("--")
        parse_long(arg, argv)
      elsif arg.start_with?("-") && arg.length > 1
        parse_short(arg, argv)
      else
        rest.push(arg)
      end
    end
    rest.each { |r| argv.push(r) }
    argv
  end

  # The non-destructive form: the caller's array is untouched.
  def parse(argv)
    parse!(argv.dup)
  end

  def help
    lines = []
    lines.push(@banner) if @banner
    @summary.each do |entry|
      if entry.is_a?(Switch)
        text = entry.summary
        unless entry.desc.empty?
          # optparse's summary_indent (4) + summary_width (32); a left column
          # that overflows the width still gets its single separating space.
          text = text + " " * (36 - text.length) if text.length < 36
          text = text + " " + entry.desc
        end
        lines.push(text)
      else
        lines.push(entry)
      end
    end
    lines.join("\n") + "\n"
  end

  def to_s
    help
  end

  private

  # "--name NAME" / "--name=NAME" -> ["--name", "NAME"]; "-v" -> ["-v", nil].
  def split_spec(spec)
    if spec.include?("=")
      parts = spec.split("=", 2)
      [parts[0], parts[1]]
    elsif spec.include?(" ")
      parts = spec.split(" ", 2)
      [parts[0], parts[1]]
    else
      [spec, nil]
    end
  end

  def find_long(name)
    @switches.each { |sw| return sw if sw.long == name }
    nil
  end

  def find_short(name)
    @switches.each { |sw| return sw if sw.short == name }
    nil
  end

  def parse_long(arg, argv)
    name = arg
    inline = nil
    if arg.include?("=")
      parts = arg.split("=", 2)
      name = parts[0]
      inline = parts[1]
    end
    sw = find_long(name)
    raise InvalidOption.new(name) unless sw
    if sw.takes_argument?
      value = inline
      value = argv.shift if value.nil?
      raise MissingArgument.new(name) if value.nil?
      sw.call(value)
    else
      sw.call(true)
    end
  end

  # Short switches bundle: `-abc` is `-a -b -c`, and the first one that takes
  # a value consumes the rest of the cluster as that value (`-nfoo`).
  def parse_short(arg, argv)
    i = 1
    while i < arg.length
      name = "-" + arg[i]
      sw = find_short(name)
      raise InvalidOption.new(name) unless sw
      if sw.takes_argument?
        value = i + 1 < arg.length ? arg[(i + 1)..-1] : argv.shift
        raise MissingArgument.new(name) if value.nil?
        sw.call(value)
        return
      end
      sw.call(true)
      i = i + 1
    end
  end
end

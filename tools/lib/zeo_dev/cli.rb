# frozen_string_literal: true

require "optparse"

module ZeoDev
  # Shared argument parsing. Every command answers `--help`; the Rust commands
  # this replaces had no argument parsing at all, so an unknown flag was
  # sometimes reported and sometimes taken as a positional.
  #
  # A command subclasses this, declares its flags in `options`, and implements
  # `run`. The dispatcher does the rest.
  class Cli
    attr_reader :opts, :args

    # `usage: zeo-dev <name> ...` -- set by each subclass.
    def self.banner = "usage: zeo-dev #{command_name}"

    # `GemProbe` -> `gem-probe`.
    def self.command_name
      name.split("::").last.gsub(/([a-z])([A-Z])/, '\1-\2').downcase
    end

    # One line for the dispatcher's own `--help`.
    def self.summary = ""

    def initialize
      @opts = defaults
      @args = []
    end

    # Overridden by a subclass that has any.
    def defaults = {}

    # Overridden by a subclass to declare its flags on `parser`.
    def options(parser); end

    def parse!(argv)
      parser = OptionParser.new do |o|
        o.banner = self.class.banner
        options(o)
        o.on("-h", "--help", "print this message") do
          puts o
          return :help
        end
      end
      @args = parser.parse(argv)
      self
    rescue OptionParser::ParseError => e
      raise Error, "#{self.class.command_name}: #{e.message}"
    end

    # The command's work. Answers a process exit status.
    def run = raise(Error, "#{self.class.command_name} has no run")

    # `zeo-dev <name> ...` from anywhere: run the whole command.
    def self.main(argv)
      cli = new
      return 0 if cli.parse!(argv) == :help

      cli.run
    end

    private

    # A command that takes no positional argument says so rather than
    # ignoring one.
    def expect_no_args!
      return if @args.empty?

      raise Error, "#{self.class.command_name}: unexpected argument #{@args.first.inspect}"
    end
  end
end

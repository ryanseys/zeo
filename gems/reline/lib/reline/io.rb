
# zeo: `reline/io/ansi` is required at the top rather than inside
# `decide_io_gate`. A whole-program AOT compile does not load a library a
# method body alone requires, and this is the gate every non-dumb terminal
# goes through. The Windows require below stays where it is: that branch is
# unreachable on the platforms zeo builds for.
require 'reline/io/ansi'

module Reline
  class IO
    RESET_COLOR = "\e[0m"

    def self.decide_io_gate
      if ENV['TERM'] == 'dumb'
        Reline::Dumb.new
      else
        case RbConfig::CONFIG['host_os']
        when /mswin|msys|mingw|cygwin|bccwin|wince|emc/
          require 'reline/io/windows'
          io = Reline::Windows.new
          if io.msys_tty?
            Reline::ANSI.new
          else
            io
          end
        else
          Reline::ANSI.new
        end
      end
    end

    def dumb?
      false
    end

    def win?
      false
    end

    def reset_color_sequence
      self.class::RESET_COLOR
    end

    # Read a single encoding valid character from the input.
    def read_single_char(timeout_second)
      buffer = String.new(encoding: Encoding::ASCII_8BIT)
      loop do
        timeout = buffer.empty? ? Float::INFINITY : timeout_second
        c = getc(timeout)
        return unless c

        buffer << c
        encoded = buffer.dup.force_encoding(encoding)
        return encoded if encoded.valid_encoding?
      end
    end
  end
end

require 'reline/io/dumb'

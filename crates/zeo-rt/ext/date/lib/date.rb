# frozen_string_literal: true
# date.rb: Written by Tadayoshi Funaba 1998-2011

# zeo: pull in the statically linked native half FIRST, so `Date` exists to be
# reopened below -- zeo's loader idiom (see ext/strscan/lib/strscan.rb).
# Upstream's own half opens with `require 'date_core'`, which reaches the same
# C extension by its build name.
require "date.so"

class Date
  # zeo: `VERSION` comes from the native half, which already declares it.

  # zeo: CRuby defines this in C (`date_core.c`'s `eDateError`), but a feature-gated
  # native class cannot register a constructible exception in this runtime: an
  # ABI row is gated-but-constructor-less, and the exception table is
  # constructible-but-ungated, so no row shape is both. Defining it here makes
  # it an ordinary user class -- registered under its fully qualified name with
  # a real constructor -- which the native half then raises by name. Same split
  # as `StringScanner::Error`.
  class Error < ArgumentError; end

  # call-seq:
  #   infinite? -> false
  #
  # Returns +false+
  def infinite?
    false
  end

  class Infinity < Numeric # :nodoc:

    def initialize(d=1) @d = d <=> 0 end

    def d() @d end

    protected :d

    def zero?() false end
    def finite?() false end
    def infinite?() d.nonzero? end
    def nan?() d.zero? end

    def abs() self.class.new end

    def -@() self.class.new(-d) end
    def +@() self.class.new(+d) end

    def <=>(other)
      case other
      when Infinity; return d <=> other.d
      when Float::INFINITY; return d <=> 1
      when -Float::INFINITY; return d <=> -1
      when Numeric; return d
      else
        begin
          l, r = other.coerce(self)
          return l <=> r
        rescue NoMethodError
        end
      end
      nil
    end

    def coerce(other)
      case other
      when Numeric; return -d, d
      else
        super
      end
    end

    def to_f
      return 0 if @d == 0
      if @d > 0
        Float::INFINITY
      else
        -Float::INFINITY
      end
    end

  end

end

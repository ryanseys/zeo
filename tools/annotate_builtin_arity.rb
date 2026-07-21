#!/usr/bin/env ruby
# frozen_string_literal: true

# Re-declares per-name Method#arity annotations on the value-class
# `builtin_methods!` tables, verified against this ruby's own reflection (run it
# under the 4.0.5 oracle).
#
# Each builtin row's name literal carries an optional `[n]` arity suffix (CRuby's
# `rb_define_method` argc, per-name since aliases can diverge -- Array#<< is 1 but
# #push is -1). This script rewrites those suffixes to match
# `Klass.instance_method(name).arity`, so the declarations stay honest.
#
# Run from the repo root under the oracle:
#   ~/.local/share/mise/installs/ruby/4.0.5/bin/ruby tools/annotate_builtin_arity.rb

require "set"

FILES = {
  "integer.rs" => Integer, "float.rs" => Float, "numeric.rs" => Numeric,
  "rational.rs" => Rational, "complex.rs" => Complex, "string.rs" => String,
  "symbol.rs" => Symbol, "array.rs" => Array, "hash.rs" => Hash,
  "range.rs" => Range, "regexp.rs" => Regexp, "matchdata.rs" => MatchData,
  "set.rs" => Set, "time.rs" => Time, "comparable.rs" => Comparable,
  "enumerable.rs" => Enumerable
}.freeze

BASE = "crates/spinel-rt/src/builtins/"

# The names portion of a row: literals (each optionally already `[n]`) up to
# `=> fn`. Capture 1 is exactly that portion, so only it is rewritten.
ROW = /^\s*((?:"(?:[^"\\]|\\.)*"(?:\[-?\d+\])?\s*\|\s*)*"(?:[^"\\]|\\.)*"(?:\[-?\d+\])?)\s*=>\s*fn\s/
NAME = /"((?:[^"\\]|\\.)*)"(\[-?\d+\])?/

def arity_of(klass, name)
  sym = name.to_sym
  klass.instance_methods.include?(sym) ? klass.instance_method(sym).arity : nil
rescue StandardError
  nil
end

edits = 0
FILES.each do |file, klass|
  path = BASE + file
  next unless File.exist?(path)

  lines = File.read(path).split("\n", -1)
  lines.each_with_index do |line, i|
    m = ROW.match(line)
    next unless m

    names = m[1].gsub(NAME) do
      name = Regexp.last_match(1)            # method names here are escape-free
      a = arity_of(klass, name)
      if a.nil? || a == -1
        %("#{name}")                          # drop any stale suffix -> -1 default
      else
        edits += 1
        %("#{name}"[#{a}])
      end
    end
    lines[i] = line[0...m.begin(1)] + names + line[m.end(1)..]
  end
  File.write(path, lines.join("\n"))
end

puts "annotated #{edits} names"

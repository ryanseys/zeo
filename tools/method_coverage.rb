#!/usr/bin/env ruby
# frozen_string_literal: true

# Differential method-coverage between `ruby` (the oracle on PATH) and
# `zeo -e`. For each core class we take a representative sample instance,
# ask Ruby for the full public method surface it responds to, then run the same
# `respond_to?` probe through zeo and report every method Ruby has that
# zeo is missing (or errors on).
#
# Usage:
#   ruby tools/method_coverage.rb [Class ...]     # default: all classes below
#   ZEO=path/to/zeo ruby tools/method_coverage.rb String
#
# One compiled zeo program per class (not per method), so a full sweep is
# ~a dozen rustc builds, not thousands.

ZEO = ENV.fetch("ZEO", "target/debug/zeo")

# class name => a literal expression evaluating to a representative instance.
SAMPLES = {
  "Integer"    => "42",
  "Float"      => "3.14",
  "String"     => '"sample"',
  "Symbol"     => ":sample",
  "Array"      => "[1, 2, 3]",
  "Hash"       => "{ a: 1 }",
  "Range"      => "(1..5)",
  "Regexp"     => "/re/",
  "NilClass"   => "nil",
  "TrueClass"  => "true",
  "FalseClass" => "false",
  "Proc"       => "->(x) { x }",
  "Rational"   => "Rational(1, 2)",
  "Complex"    => "Complex(1, 2)",
}.freeze

# Runs `code` through `argv` (a command prefix), returning [stdout_lines_hash].
# The probe prints `name\tbool` per line; we parse into { name => "true"/"false" }.
def probe_results(argv, code)
  out = IO.popen([*argv, code], err: %i[child out], &:read)
  ok = $?.success?
  parsed = {}
  out.each_line do |line|
    name, val = line.chomp.split("\t", 2)
    parsed[name] = val if name && val
  end
  [parsed, ok, out]
end

def probe_program(sample, method_names)
  <<~PROG
    sample = #{sample}
    #{method_names.inspect}.each do |m|
      puts "\#{m}\t\#{sample.respond_to?(m.to_sym)}"
    end
  PROG
end

targets = ARGV.empty? ? SAMPLES.keys : ARGV
grand_missing = 0

targets.each do |cls|
  sample = SAMPLES[cls]
  unless sample
    warn "skip #{cls}: no sample instance defined"
    next
  end

  # Ruby's own view of the sample's public method surface.
  names = eval(sample).public_methods.map(&:to_s).uniq.sort
  program = probe_program(sample, names)

  ruby_res, = probe_results(["ruby", "-e"], program)
  spin_res, spin_ok, spin_out = probe_results([ZEO, "-e"], program)

  unless spin_ok && !spin_res.empty?
    puts "#{cls}: zeo failed to run the probe (compile error or crash):"
    puts spin_out.lines.first(6).map { |l| "    #{l}" }.join
    puts
    next
  end

  missing = names.select { |m| ruby_res[m] == "true" && spin_res[m] != "true" }
  puts "== #{cls} == #{names.size} public methods, #{missing.size} missing in zeo"
  unless missing.empty?
    missing.each_slice(6) { |row| puts "    #{row.join(', ')}" }
  end
  grand_missing += missing.size
  puts
end

puts "TOTAL missing across #{targets.size} class(es): #{grand_missing}"

# frozen_string_literal: true

require "tmpdir"

module ZeoDev
  module Commands
    # What each builtin class table costs a linked binary.
    #
    # Every size figure in the docs before this was prose. This measures: link
    # `puts 1`, then link it again once per table with that table dropped
    # (`ZEO_DEBUG_DROP_TABLE`), and diff. That is exact per-table attribution,
    # and there is no other way to get it -- a table roots its class's whole
    # method surface, so `Regexp`'s drags in two regex engines and
    # `RubyVM::AST`'s drags in prism, and the interesting rows are the ones
    # far bigger than their own code.
    #
    # Safe precisely because a missing table is now LOUD. `registered_table`
    # aborts naming the class rather than answering `NoMethodError` for every
    # row it has, so a drop cannot make a quietly wrong program.
    class Size < Cli
      PROGRAM = "puts 1\n"

      # The committed number `--check` defends, in bytes of the whole binary.
      # Recorded 2026-08-24 on aarch64-apple-darwin, release profile, after
      # `analyze::class_reach` narrowed the always-on tables to the set a
      # program can reach.
      SIZE_BASELINE = 7_493_552

      def self.summary = "what each builtin class table costs a linked binary"

      def self.banner = <<~TEXT
        usage: zeo-dev size [--check] [--tolerance BYTES] [--top N]

        With no flags: link `puts 1`, then link it once per class table with
        that table dropped, and print what each one costs.

        `--check` links the baseline only and fails if it moved -- the CI gate,
        so a size regression is loud rather than prose.
      TEXT

      def defaults = { tolerance: 262_144, top: 25 }

      def options(o)
        o.on("--check", "fail if the baseline moved past --tolerance") { opts[:check] = true }
        o.on("--tolerance BYTES", Integer, "allowed drift (default 262144)") { |v| opts[:tolerance] = v }
        o.on("--top N", Integer, "how many rows to show (default 25)") { |v| opts[:top] = v }
      end

      def run
        zeo = build_zeo
        Dir.mktmpdir("zeo-size") do |tmp|
          src = File.join(tmp, "hello.rb")
          File.write(src, PROGRAM)
          base = link(zeo, src, File.join(tmp, "base"), nil)
          raise Error, "size: the baseline program did not link" unless base
          return check(base) if opts[:check]

          puts "baseline (`puts 1`): #{comma(base)} bytes"
          rows = tables.filter_map do |sym|
            bytes = link(zeo, src, File.join(tmp, "drop"), sym)
            bytes && [sym.delete_prefix("zeo_ctable_"), base - bytes]
          end
          report(rows.sort_by { |_, cost| -cost })
        end
        0
      end

      private

      def check(base)
        drift = base - SIZE_BASELINE
        sign = drift.negative? ? "" : "+"
        if drift.abs > opts[:tolerance]
          warn "size: `puts 1` is #{comma(base)} bytes, #{sign}#{comma(drift)} against the " \
               "#{comma(SIZE_BASELINE)} baseline (tolerance #{comma(opts[:tolerance])}). " \
               "If this is intended, move SIZE_BASELINE in " \
               "tools/lib/zeo_dev/commands/size.rb and say why in the commit."
          return 1
        end
        puts "size: `puts 1` is #{comma(base)} bytes (#{sign}#{comma(drift)})"
        0
      end

      def report(rows)
        total = rows.sum { |_, cost| cost }
        puts
        puts format("%-46s %12s", "table", "bytes")
        rows.first(opts[:top]).each { |name, cost| puts format("%-46s %12s", name, comma(cost)) }
        puts
        puts "#{rows.size} tables, #{comma(total)} bytes attributed -- the columns OVERLAP, " \
             "because two tables can root the same code."
      end

      # Every table symbol the compiler knows, read out of the projection its
      # own build script writes: the same list `needed_class_tables` filters.
      def tables
        generated = Dir.glob(File.join(ROOT, "target/*/build/zeo-*/out/class_surface.rs"))
        text = generated.map { |f| File.read(f) }.join
        raise Error, "no generated class surface -- run `cargo build -p zeo`" if text.empty?

        text.scan(/"(zeo_ctable_[A-Z0-9_]+)"/).flatten.uniq.sort
      end

      def build_zeo
        res = Exec.run(%w[cargo build --release -p zeo], chdir: ROOT)
        raise Error, "size: `cargo build --release -p zeo` failed" unless res.success?

        File.join(ROOT, "target/release/zeo")
      end

      # Link `src` with `drop` excluded; the artifact's size, or `nil` when
      # the link failed -- which is a real answer for a table whose absence
      # the runtime's own ICE catches.
      def link(zeo, src, out, drop)
        env = drop ? { "ZEO_DEBUG_DROP_TABLE" => drop } : {}
        res = Exec.run([zeo, "-o", out, src], env: env, chdir: ROOT)
        return nil unless res.success? && File.exist?(out)

        size = File.size(out)
        File.delete(out)
        size
      end

      def comma(n)
        sign = n.negative? ? "-" : ""
        "#{sign}#{n.abs.to_s.reverse.scan(/\d{1,3}/).join(",").reverse}"
      end
    end
  end
end

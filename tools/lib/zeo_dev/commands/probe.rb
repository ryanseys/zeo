# frozen_string_literal: true

module ZeoDev
  module Commands
    # Run a differential PROBE MATRIX through both engines and report only the
    # rows that disagree.
    #
    # `zeo-dev diff` compiles one snippet per invocation, which is the wrong
    # shape for a matrix of hundreds of rows. A probe is one Ruby program
    # holding the whole matrix, each row printing `name<TAB>result`: one
    # compile, hundreds of rows, and the divergent ones come back grouped and
    # named.
    #
    # The matrices live in `tools/probes/`. Adding a row costs one line, and a
    # row that AGREES is regression cover the moment someone breaks it.
    class Probe < Cli
      def self.summary = "run a differential probe matrix across ruby and zeo"

      def self.banner = <<~TEXT
        usage: zeo-dev probe [<name>...]

          zeo-dev probe                 # every matrix in tools/probes/
          zeo-dev probe arguments       # one of them
          zeo-dev probe --all-rows      # show agreeing rows too
      TEXT

      def defaults = { all_rows: false }

      def options(o)
        o.on("--all-rows", "print every row, not only the divergent ones") { opts[:all_rows] = true }
      end

      def run
        names = args.empty? ? available : args
        missing = names.reject { |n| File.exist?(path_of(n)) }
        raise Error, "no such probe: #{missing.join(", ")} (have: #{available.join(", ")})" if missing.any?

        zeo = Ruby.build_zeo!
        total = names.sum { |n| run_one(n, zeo) }
        puts
        puts total.zero? ? "no divergent rows" : "#{total} divergent row(s)"
        total.zero? ? 0 : 1
      end

      private

      def available
        Dir.glob(File.join(ROOT, "tools", "probes", "*.rb")).map { |p| File.basename(p, ".rb") }.sort
      end

      def path_of(name) = File.join(ROOT, "tools", "probes", "#{name.delete_suffix(".rb")}.rb")

      def run_one(name, zeo)
        file = path_of(name)
        puts "=== #{name} ==="
        ruby = Exec.run(Ruby.oracle_argv(file), capture_stdout: true, chdir: ROOT)
        mine = Exec.run([zeo, "-W0", file], capture_stdout: true, chdir: ROOT)
        report_engine_failure("ruby", ruby)
        report_engine_failure("zeo", mine)

        rows_r = rows(ruby.stdout)
        rows_z = rows(mine.stdout)
        order = rows_r.keys | rows_z.keys
        diverged = order.reject { |k| rows_r[k] == rows_z[k] }
        order.each do |k|
          next unless opts[:all_rows] || diverged.include?(k)

          mark = diverged.include?(k) ? "!" : " "
          puts "#{mark} #{k}"
          puts "    ruby: #{rows_r.fetch(k, "<no row>")}"
          puts "    zeo : #{rows_z.fetch(k, "<no row>")}"
        end
        puts "  #{diverged.size} of #{order.size} rows diverge"
        diverged.size
      end

      # A probe that dies part way through still reports what it printed, so
      # the rows before the crash stay usable -- but the crash itself is the
      # most important finding on the page, so it is never swallowed, and
      # every `<no row>` below it is a CONSEQUENCE rather than a finding of
      # its own. Say that out loud: a truncated sweep reads exactly like a
      # large genuine cluster, and has been mistaken for one.
      def report_engine_failure(label, res)
        return if res.code == 0 && res.stderr.to_s.empty?

        status = res.timed_out ? "timed out" : "exited #{res.code.inspect}"
        puts "  !! #{label} #{status} -- rows after the failure are MISSING, not divergent"
        res.stderr.to_s.lines.first(6).each { |l| puts "     #{l.chomp}" }
      end

      # The same address scrub the golden harness applies. Without it every
      # row whose text embeds an object address reads as a divergence, which
      # buries the real ones.
      ADDRESS = /0x[0-9a-f]{8,16}/

      def rows(text)
        text.to_s.lines.each_with_object({}) do |line, acc|
          name, _, result = line.chomp.partition("\t")
          acc[name] = result.gsub(ADDRESS, "0xADDR") unless result.empty?
        end
      end
    end
  end
end

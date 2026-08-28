# frozen_string_literal: true

require "fileutils"
require "tmpdir"

module ZeoDev
  module Commands
    # Run a ruby snippet through BOTH engines, show each side, and say whether
    # they agree. On a DIVERGENCE it captures the snippet as an XFAIL gap under
    # `tests/gaps/`, blessed from ruby; on a MATCH it writes nothing.
    #
    # This is the long-tail loop in one command: find a divergence, file it
    # with its golden, confirm the harness agrees it is real.
    #
    # The gaps harness is the ARBITER of whether a divergence is a real gap.
    # It normalizes source paths, so two outputs that differ here can still
    # match there -- after writing and blessing, the gap is run, and a
    # "GAP FIXED" verdict means the file is removed and the snippet belongs in
    # `tests/` instead.
    class Diff < Cli
      def self.summary = "compare a snippet across ruby and zeo, and file a gap"

      def self.banner = <<~TEXT
        usage: zeo-dev diff [<code> | -f <file> | -]

          zeo-dev diff 'puts 1 + 1'          # inline code
          zeo-dev diff -f snippet.rb         # from a file
          pbpaste | zeo-dev diff -           # from stdin
      TEXT

      def defaults = { name: "gap_snippet", gap: true, file: nil, stdin: false }

      def options(o)
        o.on("--name NAME", "-n NAME", "gap stem to use on divergence") { |v| opts[:name] = v }
        o.on("--no-gap", "compare only; never write a gap file") { opts[:gap] = false }
        o.on("-f FILE", "read the snippet from FILE") { |v| opts[:file] = v }
      end

      def run
        source = read_source
        raise Error, self.class.banner if source.nil?

        puts "=== snippet ==="
        puts source
        puts

        ruby = Exec.run(Ruby.oracle_argv("-"), env: Ruby.oracle_env, stdin: source, capture_stdout: true, chdir: ROOT)
        zeo = Exec.run([Ruby.build_zeo!, "-W0", "-e", source], capture_stdout: true, chdir: ROOT)
        show("ruby", ruby)
        puts
        show("zeo", zeo)
        puts

        # A quick verdict on stdout plus exit; the harness reconciles the fine
        # print.
        if ruby.stdout == zeo.stdout && ruby.code == zeo.code
          puts "MATCH (stdout + exit). Not a gap -- belongs in tests/ if you want to keep it."
          return 0
        end
        puts "DIVERGE:"
        puts "  ruby exit #{ruby.code.inspect}, zeo exit #{zeo.code.inspect}" if ruby.code != zeo.code
        show_stdout_diff(ruby.stdout, zeo.stdout)
        puts
        return puts("(--no-gap: not writing a gap file)") || 0 unless opts[:gap]

        write_gap(source)
      end

      private

      def read_source
        return File.read(opts[:file]) if opts[:file]
        return $stdin.read if args.first == "-" || (args.empty? && !$stdin.tty?)

        args.shift&.then { |s| "#{s}\n" }
      end

      def show(label, res)
        puts "--- #{label} (exit #{res.code.inspect}) ---"
        puts "stdout:"
        print res.stdout
        return if res.stderr.to_s.empty?

        puts "stderr:"
        print res.stderr
      end

      def show_stdout_diff(a, b)
        al = a.to_s.lines
        bl = b.to_s.lines
        [al.size, bl.size].max.times do |i|
          next if al[i] == bl[i]

          puts "  ruby #{i + 1}: #{al[i].inspect}"
          puts "  zeo  #{i + 1}: #{bl[i].inspect}"
        end
      end

      def write_gap(source)
        name = opts[:name].delete_suffix(".rb")
        dest = File.join(ROOT, "tests", "gaps", "#{name}.rb")
        raise Error, "refusing to overwrite tests/gaps/#{name}.rb (use --name)" if File.exist?(dest)

        File.write(dest, source)
        bless_gap(name)
        puts "wrote tests/gaps/#{name}.rb (+ blessed golden from ruby)"

        puts "confirming the XFAIL holds via the gaps harness ..."
        run_out = Exec.run(%W[cargo nextest run -p zeo --test goldens -E test(#{name})],
                           chdir: ROOT, capture_stdout: true)
        if "#{run_out.stdout}#{run_out.stderr}".include?("GAP FIXED")
          puts
          puts "the gaps harness says zeo actually MATCHES ruby (under source-path"
          puts "normalization) -- this is NOT a gap. Removing it; promote to tests/ instead."
          %w[rb rb.expected rb.err.expected].each do |suf|
            FileUtils.rm_f(File.join(ROOT, "tests", "gaps", "#{name}.#{suf}"))
          end
          return 1
        end
        puts "gap tests/gaps/#{name}.rb is a valid XFAIL (zeo diverges). Grind it down, then:"
        puts "  tools/zeo-dev promote-gap #{name}"
        0
      end

      # Delegates to `zeo-dev bless` -- the ONE golden writer -- so the
      # recorded output goes through the harness's own normalization (CRLF,
      # source-path relativization, address scrubbing). A hand-rolled
      # oracle capture here once skipped the address scrub, so a snippet
      # printing `#<Object:0x...>` recorded a raw process-random address
      # into its golden.
      def bless_gap(name)
        res = Exec.run([File.join(ROOT, "tools", "zeo-dev"), "bless", name],
                       chdir: ROOT, capture_stdout: true)
        raise Error, "bless failed for #{name}:\n#{res.stderr}" unless res.success?
      end
    end
  end
end

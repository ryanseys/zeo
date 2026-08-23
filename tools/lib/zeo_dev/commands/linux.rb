# frozen_string_literal: true

require "fileutils"

module ZeoDev
  module Commands
    # Run zeo's suites on Linux, in the container `scripts/linux/Dockerfile`
    # builds. The stand-in for a CI leg, and the only place the Linux link
    # path is exercised at all.
    #
    # Every stage tees its output to `scripts/linux/logs/<stage>.log`. The
    # container runs `--rm`, so its own logs die with it -- a run whose output
    # only existed there could not be read back.
    #
    # Every stage needs `build` to have run first: nothing rebuilds the `zeo`
    # BINARY for a test target. The target volume persists between runs, so a
    # second `build` is incremental.
    #
    # The container runs as ROOT, and one golden can tell:
    # `process_identity_rows` expects `Errno::EPERM` from `Sys.setuid("root")`
    # and its siblings, which succeed here. CI and any developer machine run
    # unprivileged, so the expectation is right and this leg is the odd one
    # out -- do not bless it away.
    class Linux < Cli
      DEFAULT_STAGES = %w[build jit aot units natlibs valgrind].freeze
      STAGES = (DEFAULT_STAGES + %w[cross shell all]).freeze

      def self.summary = "run the suites in the linux container"

      def self.banner = <<~TEXT
        usage: zeo-dev linux <stage>... | -E '<nextest expr>'

        stages:
          build     cargo build --workspace
          jit       the default golden legs
          aot       the same corpora, linked binaries
          units     zeo + zeo-rt unit suites
          natlibs   diff link.rs's glibc table against rustc
          valgrind  leak-check a linked program
          cross     x86_64 compile check
          shell     an interactive prompt in the image
          all       #{DEFAULT_STAGES.join(" ")}

        -E '<expr>' runs one nextest filter, for triage after a red run.
      TEXT

      def defaults = { filter: nil }

      def options(o)
        o.on("-E EXPR", "run one nextest filter expression") { |v| opts[:filter] = v }
      end

      def run
        return stage("-E") if opts[:filter]

        wanted = args.empty? || args == ["all"] ? DEFAULT_STAGES : args
        unknown = wanted - STAGES
        raise Error, "unknown stage: #{unknown.first}" unless unknown.empty?

        wanted.each do |s|
          code = stage(s)
          return code unless code.zero?
        end
        0
      end

      private

      # RELEASE, not dev. Every AOT golden LINKS a whole binary against
      # `libzeo.a`, and the dev archive is 315 MB against release's 87 MB --
      # measured, a hello compile+link is 1.48s dev and 0.69s release, and
      # this leg pays that 4,061 times. `ZEO_CLIF_VERIFY=1` keeps what
      # `debug_assertions` was buying here: the Cranelift verifier and the
      # ownership ledger.
      def profile = ENV["ZEO_LINUX_PROFILE"] || "release"
      def cargo_profile = profile == "release" ? "--release" : ""
      # cargo's dev profile writes to `debug/`, which is why the two names
      # cannot be one value.
      def target_dir = profile == "release" ? "release" : "debug"

      def image = ENV["ZEO_LINUX_IMAGE"] || "zeo-linux"
      # The VM's own width. The e2e tier LINKS a whole binary per test, so
      # this run is bound by `cc` far more than by the compiler -- threads are
      # the lever that matters, and the ceiling is the podman machine's cpu
      # count (`podman machine set --cpus N`, which needs the machine
      # stopped).
      def threads = ENV["ZEO_LINUX_THREADS"] || "8"
      def memory = ENV["ZEO_LINUX_MEMORY"] || "10g"
      def volume = ENV["ZEO_LINUX_VOLUME"] || "zeo-linux-target"
      def engine = ENV["ZEO_CONTAINER_ENGINE"] || "podman"
      def logs = File.join(ROOT, "scripts", "linux", "logs")

      def stage(name)
        tag = name.gsub(/[^A-Za-z0-9_.-]+/, "_")
        FileUtils.mkdir_p(logs)
        puts "=== linux: #{name}  (log: scripts/linux/logs/#{tag}.log)"
        run_in_container(script_for(name), tag)
      end

      def script_for(name)
        case name
        when "build" then "cargo build --workspace #{cargo_profile}"
        # No env var = the default (jit) leg. The goldens spawn a child zeo
        # each and the watchdog caps them at 512 MiB.
        when "jit"
          "cargo nextest run #{cargo_profile} -p zeo-tests --test-threads #{threads} --no-fail-fast"
        when "aot"
          "ZEO_GOLDEN_BACKEND=aot cargo nextest run #{cargo_profile} -p zeo-tests " \
            "--test-threads #{threads} --no-fail-fast --test examples --test spinel --test gaps"
        when "units"
          "cargo nextest run #{cargo_profile} -p zeo -p zeo-rt --test-threads #{threads}"
        # The one test that asks rustc for the live answer instead of trusting
        # the table; ignored by default because it compiles the lib in a probe
        # target dir. This is the platform whose table was never confirmed.
        when "natlibs"
          "cargo test -p zeo --lib -- --ignored natlibs_table_matches_rustc --nocapture"
        when "valgrind" then valgrind_script
        when "cross" then cross_script
        when "shell" then "exec bash"
        when "-E"
          "cargo nextest run #{cargo_profile} -p zeo-tests --test-threads #{threads} " \
            "--no-fail-fast -E '#{opts[:filter]}'"
        else raise Error, "unknown stage: #{name}"
        end
      end

      def valgrind_script
        <<~SH
          set -e
          cd /tmp && printf "%s\\n" "class P; def initialize(n) = @n = n; def to_s = \\"P(\#{@n})\\"; end" \\
            "10.times { |i| puts P.new(i) }" "a = (1..50).map { |i| i * i }; puts a.sum" \\
            "begin; raise ArgumentError, \\"x\\"; rescue => e; puts e.message; end" > vg.rb
          /target/#{target_dir}/zeo vg.rb -o vg
          valgrind --error-exitcode=9 --leak-check=full --errors-for-leak-kinds=definite ./vg
        SH
      end

      def cross_script
        <<~SH
          rustup target add x86_64-unknown-linux-gnu
          CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc \\
          CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc \\
          AR_x86_64_unknown_linux_gnu=x86_64-linux-gnu-ar \\
          cargo check --workspace --target x86_64-unknown-linux-gnu
        SH
      end

      # `--platform`: see the Dockerfile header. `--memory`: the golden
      # harness's own RSS watchdog assumes room to work.
      #
      # The repo mounts READ-WRITE, and that is deliberate rather than lazy: a
      # golden runs with its cwd set to `tests/`, and a dozen of them create a
      # temp file there on purpose -- the oracle did exactly that when the
      # `.expected` was blessed. A read-only mount turns every one of those
      # into `Errno::EROFS`, which reads as a zeo bug and is not one.
      #
      # `--pids-limit`: podman defaults to 2048, and the thread goldens spawn
      # enough OS threads at eight-way parallelism to hit it --
      # `pthread_create` then fails EAGAIN and the runtime panics mid-test.
      # Another failure that is the harness, not the program.
      #
      # `-it` only when there IS a terminal: the same command runs from a
      # non-tty caller (CI, an agent), where podman refuses the flag.
      def run_in_container(script, tag)
        argv = [engine, "run", "--rm"]
        argv << "-it" if $stdin.tty? && $stdout.tty?
        argv.push("--platform", "linux/arm64",
                  "--memory", memory,
                  "-e", "ZEO_CLIF_VERIFY=1",
                  "--pids-limit", "16384",
                  "-v", "#{ROOT}:/src", "-v", "#{volume}:/target", "-v", "#{logs}:/logs",
                  "-w", "/src", image, "bash", "-c", script)
        log = File.join(logs, "#{tag}.log")
        # `bash -c`, never `-lc`: a login shell re-reads the image's profile
        # and loses the environment set on the run.
        code = tee(argv, log)
        warn "linux: #{tag} exited #{code}" unless code.zero?
        code
      end

      # Streams to the terminal AND the log. The container is `--rm`, so its
      # own output is the only copy.
      def tee(argv, log)
        File.open(log, "wb") do |f|
          IO.popen(argv, err: [:child, :out]) do |io|
            io.each_line do |line|
              print line
              $stdout.flush
              f.write(line)
            end
          end
        end
        $?.exitstatus
      end
    end
  end
end

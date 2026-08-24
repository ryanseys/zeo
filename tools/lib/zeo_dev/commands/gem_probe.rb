# frozen_string_literal: true

require "digest"
require "fileutils"
require "json"
require "net/http"
require "stringio"
require "rubygems/package"
require "tmpdir"
require "uri"
require "yaml"
require "zlib"

module ZeoDev
  module Commands
    # Fetch a gem from rubygems.org, try to compile it, and record the verdict.
    #
    # This answers a question a LAYOUT classification cannot. `zeo::gem_compat`
    # reads a gem as "pure Ruby, so zeo would compile it". It never runs the
    # compiler, so a gem using a construct zeo cannot lower still reports as
    # resolvable -- concurrent-ruby is the standing example. This runs the
    # front end for real.
    #
    # Four steps, and only the first touches the network:
    #
    #   resolve  name [version]     -> an exact version
    #   fetch    the .gem           -> vendor/gems/<name>/   (cached)
    #   probe    require "<entry>"  -> a stage and an outcome
    #   record   the verdict        -> measurements/gem-probe.tsv
    #
    # Probing an unpacked tree needs no network and is deterministic, so a
    # ledger row reproduces from its recorded version alone.
    #
    # Two columns carry the verdict and are read TOGETHER. `stage` is the rung
    # the compile reached; `outcome` is what happened there. `codegen ok` and
    # `codegen lowering-gap` are the same rung with opposite results.
    #
    # `codegen` is the last rung a sweep reaches on its own, and reaching it is
    # deliberately the weakest useful claim: no binary exists, and the gem's
    # own code may not have been compiled at all -- zeo can decline a unit and
    # defer it to a runtime `LoadError`.
    #
    # The ledger is NOT committed (21 MB) and is not a maintained database.
    # It is the output of a sweep, and a sweep is run deliberately.
    class GemProbe < Cli
      REGISTRY = "https://rubygems.org"
      LEDGER = "measurements/gem-probe.tsv"

      # Long enough that a rails-scale require graph finishes -- those take
      # minutes in the front end alone -- and short enough that a gem which
      # will never finish cannot hold a sweep open.
      DEFAULT_TIMEOUT = 600

      COLUMNS = %w[gem version stage outcome rust_bytes binary_bytes detail where sha256].freeze

      # The rungs, in order. Each is a strictly harder claim than the one
      # below. The four middle ones are zeo's own front-end passes, and a
      # rejection is recorded at the pass that made it -- which zeo already
      # prints as the diagnostic's code.
      STAGES = %w[queued fetch unpack parse lower analyze codegen build run].freeze

      def self.summary = "fetch a gem and record whether zeo compiles it"

      def self.banner = <<~TEXT
        usage: zeo-dev gem-probe [<name> [version]] [selection] [options]

        Compiles real rubygems with zeo and records how far each one got in
        measurements/gem-probe.tsv. Two columns carry the verdict and are read
        together: `stage` is the rung, `outcome` is what happened there.

        selection (at least one; they add up):
          <name> [version]   one gem; the newest version unless one is given
          --names <file>     every name in <file>, one per line

        The default stops after codegen: zeo produced CLIF, nothing was built
        and nothing was run.
      TEXT

      def defaults
        { names: nil, jobs: nil, timeout: DEFAULT_TIMEOUT,
          zeo: nil, limit: nil, refresh: false }
      end

      def options(o)
        o.on("--names FILE", "probe every name in FILE, one per line") { |v| opts[:names] = v }
        o.on("--jobs N", Integer, "gems in flight (default: derived from RAM, not cores)") do |v|
          opts[:jobs] = v
        end
        o.on("--timeout SECS", Integer, "kill a gem that outruns it (default #{DEFAULT_TIMEOUT})") do |v|
          opts[:timeout] = v
        end
        o.on("--zeo PATH", "probe with exactly this binary, and build nothing") { |v| opts[:zeo] = v }
        o.on("--limit N", Integer, "stop after N gems") { |v| opts[:limit] = v }
        o.on("--refresh", "re-probe names already in the ledger") { opts[:refresh] = true }
      end

      def run
        names = select_names
        raise Error, self.class.banner if names.empty?

        @zeo = opts[:zeo] || snapshot_binary
        @budget = Jobs.new(opts[:jobs])
        ledger = read_ledger
        # A gem named on the command line always probes. Only a bulk selection
        # skips what the ledger already carries, and `--refresh` overrides
        # that too.
        names -= (ledger.keys - @named) unless opts[:refresh]
        names = names.first(opts[:limit]) if opts[:limit]
        return puts("nothing to probe") || 0 if names.empty?

        warn "gem-probe: #{names.size} gem(s), #{@budget.describe}, zeo at #{@zeo}"
        # Resolving and fetching stay on ONE thread. They touch the network
        # and write the shared `vendor/gems` cache, and two gems routinely
        # share a dependency -- two workers unpacking the same one into the
        # same directory corrupts it. Only the compile runs wide.
        prepared = names.map { |n| prepare(n) }
        rows = @budget.each_parallel(prepared) { |r| r.is_a?(Hash) ? r : compile(*r) }
        rows.compact.each { |r| ledger[r["gem"]] = r }
        write_ledger(ledger)
        summarize(rows.compact)
        0
      end

      private

      def select_names
        names = []
        @named = []
        if (first = args.shift)
          @single_version = args.shift
          @named << first
          names << first
        end
        names.concat(read_names(opts[:names])) if opts[:names]
        names.uniq
      end

      def read_names(path)
        raise Error, "#{path} does not exist" unless File.exist?(path)

        File.readlines(path, chomp: true).map(&:strip).reject { |l| l.empty? || l.start_with?("#") }
      end

      # A sweep probes ONE binary for its whole run, and never touches cargo
      # while it does: `target/probe-bin/zeo-<sha>` is a snapshot, so a dev
      # build beside the sweep cannot change what it is measuring halfway
      # through. Run a side-by-side sweep at `--jobs 3`; the spare slot is the
      # headroom that keeps concurrent probe and dev rustc off each other.
      def snapshot_binary
        sha = `git -C #{ROOT} rev-parse --short HEAD 2>/dev/null`.strip
        sha = "dev" if sha.empty?
        dest = File.join(ROOT, "target", "probe-bin", "zeo-#{sha}")
        return dest if File.executable?(dest)

        built = Ruby.build_zeo!
        FileUtils.mkdir_p(File.dirname(dest))
        FileUtils.cp(built, dest)
        FileUtils.chmod(0o755, dest)
        dest
      end

      # ---- the four steps ----------------------------------------------

      # Resolve `name` to a version and unpack it AND its runtime dependency
      # closure. Answers `[name, version, dir, deps, sha]` for the compile
      # step, or a finished row that needs no compile.
      def prepare(name)
        version = @single_version || latest_version(name)
        meta = version_meta(name, version)
        deps = dependency_closure(name, meta)
        begin
          dir = fetch(name, version, meta["sha"])
        rescue Error => e
          # The CDN refuses `<name>-<version>.gem` with a 403 when that
          # version shipped only prebuilt platform artifacts -- there is no
          # pure-ruby gem behind the plain name at all. Ask the versions list
          # before writing `fetch-failed`, so the row names the real fact
          # about the release rather than sending readers to check the
          # network.
          platforms = e.message.include?("HTTP 403") ? ruby_platform_absent(name, version) : nil
          return row(name, version, "fetch", platforms ? "platform-gem" : "fetch-failed",
                     detail: platforms || e.message, sha256: meta["sha"])
        end
        [name, version, dir, deps, meta["sha"]]
      rescue Error => e
        row(name, version || "?", "fetch", "fetch-failed", detail: e.message)
      end

      def latest_version(name)
        v = get_json("#{REGISTRY}/api/v1/versions/#{name}/latest.json")
        version = v["version"]
        raise Error, "the registry names no version for #{name}" if version.nil? || version == "unknown"

        version
      end

      # One request answers both questions a subject needs: the digest to
      # verify the download against, and the runtime dependencies to put on
      # its load path.
      def version_meta(name, version)
        v = get_json("#{REGISTRY}/api/v2/rubygems/#{name}/versions/#{version}.json")
        { "sha" => v["sha"]&.downcase,
          "runtime" => Array(v.dig("dependencies", "runtime")).filter_map { |d| d["name"] } }
      rescue Error
        { "sha" => nil, "runtime" => [] }
      end

      # How deep the dependency walk goes, and how many gems it links into one
      # view. A real graph is shallow -- rails reaches everything it needs in
      # three hops -- and the caps exist so a pathological one cannot turn a
      # single subject into a whole-registry fetch.
      DEP_DEPTH_CAP = 6
      DEP_COUNT_CAP = 200

      # Every gem the subject needs to LOAD, not just the ones it names.
      #
      # A one-level list was the probe's largest source of FALSE VERDICTS:
      # nanoc's view carried nanoc-core but not nanoc-core's own `ddplugin`,
      # so `Nanoc::Error = Nanoc::Core::Error` never resolved and 16 gems
      # recorded a lowering-gap for a program CRuby would have stopped with a
      # LoadError. Breadth-first with a visited set, so a cycle walks once.
      #
      # A dependency is a load-path entry, not a subject: it is never
      # recorded, so its digest is never asked for. Fetches go through the
      # shared store, so a gem every subject depends on downloads once.
      def dependency_closure(name, meta)
        queue = meta["runtime"].map { |d| [d, 1] }
        seen = { name => true }
        deps = []
        until queue.empty?
          break if deps.size >= DEP_COUNT_CAP

          dep, depth = queue.shift
          next if seen[dep]

          seen[dep] = true
          begin
            version = latest_version(dep)
            fetch(dep, version, nil)
          rescue Error
            next
          end
          # Recorded even when its OWN dependencies cannot be reached: a
          # partially satisfied view is what the one-level walk always built,
          # and it is still strictly better than leaving the gem out.
          deps << dep
          next if depth >= DEP_DEPTH_CAP

          version_meta(dep, version)["runtime"].each { |n| queue << [n, depth + 1] }
        end
        deps
      end

      # When rubygems carries `version` only under non-`ruby` platforms,
      # answers the platform list; otherwise nil, INCLUDING on any network
      # failure, so a flaky request never upgrades a `fetch-failed` into a
      # claim.
      def ruby_platform_absent(name, version)
        rows = get_json("#{REGISTRY}/api/v1/versions/#{name}.json")
        platforms = Array(rows).select { |e| e["number"] == version }.filter_map { |e| e["platform"] }
        return nil if platforms.empty? || platforms.include?("ruby")

        platforms.uniq.sort.join(", ")
      rescue Error, JSON::ParserError
        nil
      end

      def fetch(name, version, expect_sha)
        dir = File.join(ROOT, "vendor", "gems", name)
        stamp = File.join(dir, ".zeo-probe-version")
        return dir if File.file?(stamp) && File.read(stamp).strip == version

        FileUtils.rm_rf(dir)
        FileUtils.mkdir_p(dir)
        bytes = get_bytes("#{REGISTRY}/downloads/#{name}-#{version}.gem")
        if expect_sha
          got = Digest::SHA256.hexdigest(bytes)
          if got != expect_sha
            raise Error, "#{name}-#{version}.gem is sha256 #{got}, but the registry describes #{expect_sha}"
          end
        end
        meta = unpack_gem(bytes, dir)
        File.write(File.join(dir, ".zeo-probe-spec"), JSON.generate(meta))
        File.write(stamp, version)
        dir
      end

      # A `.gem` is a tar of `metadata.gz`, `data.tar.gz` and
      # `checksums.yaml.gz`. The gem's own files are the middle one; the first
      # is the registry's serialized spec, which carries the `require_paths`
      # the shipped gemspec usually cannot give up statically.
      def unpack_gem(bytes, dest)
        data = nil
        metadata = nil
        ::Gem::Package::TarReader.new(StringIO.new(bytes)) do |tar|
          tar.each do |entry|
            case entry.full_name
            when "data.tar.gz" then data = entry.read
            when "metadata.gz" then metadata = entry.read
            end
          end
        end
        raise Error, "no data.tar.gz inside the .gem" if data.nil?

        ::Gem::Package::TarReader.new(Zlib::GzipReader.new(StringIO.new(data))) do |tar|
          tar.each do |entry|
            path = File.join(dest, entry.full_name)
            next unless File.expand_path(path).start_with?(File.expand_path(dest))

            if entry.directory?
              FileUtils.mkdir_p(path)
            else
              FileUtils.mkdir_p(File.dirname(path))
              File.binwrite(path, entry.read || "")
            end
          end
        end
        metadata.nil? ? default_meta : parse_metadata(Zlib::GzipReader.new(StringIO.new(metadata)).read)
      end

      def default_meta = { "require_paths" => ["lib"], "platform" => "ruby", "extensions" => [] }

      # The registry's own serialized spec. Read for three fields only, with
      # `Psych.safe_load` -- the document names ruby classes, so a full load
      # would instantiate them.
      def parse_metadata(yaml)
        doc = begin
          YAML.safe_load(yaml, permitted_classes: [Symbol], aliases: true)
        rescue StandardError
          nil
        end
        return default_meta unless doc.is_a?(Hash)

        paths = Array(doc["require_paths"]).map(&:to_s).reject(&:empty?)
        { "require_paths" => paths.empty? ? ["lib"] : paths,
          "platform" => doc["platform"].to_s.empty? ? "ruby" : doc["platform"].to_s,
          "extensions" => Array(doc["extensions"]).map(&:to_s) }
      end

      # ---- the compile --------------------------------------------------

      def compile(name, version, dir, deps, sha256)
        meta = read_meta(dir)
        if meta["platform"] != "ruby"
          return row(name, version, "fetch", "platform-gem", detail: meta["platform"], sha256: sha256)
        end

        # The gem's real load path: its declared `require_paths`, kept to the
        # directories the archive actually ships. RubyGems filters the same
        # way.
        roots = meta["require_paths"].map { |p| File.join(dir, p) }.select { |p| File.directory?(p) }
        # Every declared root missing: discover where the ruby actually lives
        # before giving up. Discovered roots ALSO ride `-I` on the compile --
        # zeo's loader honours the gemspec's (missing) require_paths, so
        # without the flag the feature would defer to a runtime require and
        # the compile would measure nothing.
        discovered = roots.empty? ? discovered_roots(dir) : []
        roots = discovered if roots.empty?
        # A gem with no load path anywhere can still publish EXECUTABLES.
        if roots.empty? && ruby_executables(dir).empty?
          return row(name, version, "unpack", rootless_outcome(dir, meta), sha256: sha256)
        end

        # A DECLARED extension is not decisive, which is why nothing checks
        # for one before this point. Many gems ship an optional C accelerator
        # beside a pure-ruby implementation and compile fine without it -- erb,
        # json, prism, bigdecimal, rbs and eight more were all reported
        # `native-extension` on the strength of the gemspec line alone, having
        # never been compiled. Try, then classify what actually failed: zeo
        # says `native (C) extension` itself when a require needs one.
        view = isolate(name, deps)

        program, = build_program(dir, roots, name)
        return row(name, version, "unpack", "no-entry-point", sha256: sha256) if program.nil?

        run_zeo(name, version, dir, view, discovered, program, sha256)
      end

      # The name ladder first; when no file carries the gem's name, require
      # every top-level file the roots ship; when there is no top-level file
      # at all (the pre-convention `lib/<dir>/` layout), the gem's own require
      # graph names its roots; and when the gem publishes NO library, its
      # ruby-shebang executables are the surface, `load`ed by absolute path
      # exactly as a RubyGems binstub runs them.
      #
      # All four keep the rule `entry_point` protects: every feature the
      # program names is a file that EXISTS, so the compile measures the gem
      # and not a guess. Without it `require "activerecord"` names no file,
      # zeo lowers an unresolvable require to a runtime `Kernel#require`
      # rather than failing, and codegen then trivially succeeds having
      # compiled none of the gem -- which is how every Rails gem once
      # reported `compiles`.
      def build_program(dir, roots, name)
        features = if (hit = entry_point(roots, name))
                     [hit]
                   else
                     tl = top_level_features(roots)
                     tl.empty? ? require_graph_root_features(roots) : tl
                   end
        return [features.map { |f| "require #{f.inspect}\n" }.join, features.first] unless features.empty?

        scripts = ruby_executables(dir)
        return [nil, nil] if scripts.empty?

        [scripts.map { |p| "load #{p.inspect}\n" }.join, File.basename(scripts.first)]
      end

      # A package directory holding ONLY this gem and its declared
      # dependencies.
      #
      # Pointing the probe at the whole of `vendor/gems` made a verdict depend
      # on which other gems happened to be cached: kramdown reported one gap
      # alone, and a different one once kramdown-parser-gfm had been fetched
      # beside it. An isolated view makes the result a function of the gem and
      # its deps, which is what the ledger claims to record.
      #
      # Outside `vendor/gems`, not under it: a view is a directory of gem
      # directories, so nesting it inside the cache would make the cache
      # contain something shaped like a gem.
      def isolate(name, deps)
        view = File.join(ROOT, "vendor", ".probe", name)
        FileUtils.rm_rf(view)
        FileUtils.mkdir_p(view)
        # DEDUPLICATED. A gem may list itself among its runtime dependencies
        # -- jeweler-generated gemspecs do it routinely -- and the registry
        # may name one twice.
        ([name] + deps).uniq.each do |gem|
          src = File.join(ROOT, "vendor", "gems", gem)
          link_gem_into_view(view, src, gem) if File.directory?(src)
        end
        view
      end

      # One gem inside a view: its own contents, symlinked, except that the
      # gemspec is replaced by a stub.
      #
      # The stub is required, not stylistic -- zeo parses gemspecs statically,
      # so the computed `s.version` most real gems use is rejected, and zeo
      # checks a gemspec's name against its directory name. What is stylistic
      # is WHERE it goes, and it goes here rather than over the real file in
      # `vendor/gems/`: overwriting made the cache no longer a copy of what
      # rubygems shipped, so a ledger row could not be reproduced from it.
      def link_gem_into_view(view, src, gem)
        dest = File.join(view, gem)
        FileUtils.mkdir_p(dest)
        Dir.children(src).sort.each do |base|
          next if base.end_with?(".gemspec")
          # The probe's own stamps describe the cache, not the gem.
          next if base.start_with?(".zeo-probe-")

          begin
            File.symlink(File.join(src, base), File.join(dest, base))
          rescue Errno::EEXIST
            # On a case-insensitive filesystem two gems whose names differ
            # only in case share one directory. `Cartesian` depends on
            # `cartesian`, and macOS cannot hold both.
            raise Error, "`#{gem}` is already in the view -- on a case-insensitive filesystem " \
                         "it cannot be told from a dependency spelled differently only in case"
          end
        end
        meta = read_meta(src)
        write_stub_gemspec(dest, gem, unpacked_version(src), meta["require_paths"])
      end

      def unpacked_version(dir)
        File.read(File.join(dir, ".zeo-probe-version")).strip
      rescue SystemCallError
        "0"
      end

      def run_zeo(name, version, dir, view, discovered, program, sha256)
        Dir.mktmpdir("zeo-probe") do |tmp|
          clif = File.join(tmp, "probe.clif")
          argv = [@zeo]
          discovered.each { |r| argv.push("-I", r) }
          # `-e` has no input path, so zeo adds no package dir of its own and
          # the isolated view stays the whole world. The repo's own `gems/`
          # comes too: a probed gem may require a stdlib feature, and
          # answering that from zeo's bundled copy is what a real compile
          # would do. The subject is the distinguished root
          # (Bundler-root semantics): a feature it provides resolves to IT,
          # never to an alphabetically earlier dependency squatting the path.
          argv.push("-e", program,
                    "--gems", view,
                    "--gems", File.join(ROOT, "gems"),
                    "--root-gem", name,
                    "-W0", "--emit-clif=#{clif}")
          res = Exec.run(argv, env: @budget.env, timeout: opts[:timeout], chdir: ROOT)

          if res.timed_out
            # A stall is the ABSENCE of a verdict. Recording it as a lowering
            # gap would put a diagnosis in the ledger that zeo never made.
            return row(name, version, "codegen", "timeout", sha256: sha256)
          end
          if res.success?
            bytes = File.exist?(clif) ? File.size(clif) : 0
            return row(name, version, "codegen", "ok", rust_bytes: bytes, sha256: sha256)
          end
          text = res.stderr.to_s.dup.force_encoding(Encoding::UTF_8).scrub
          stage, outcome, detail = classify(text, dir)
          site = site_of(text)
          # A gem the verdict points INTO that cannot run without a compiled
          # half makes the diagnostic a fact about the gem, not a zeo gap.
          if outcome == "lowering-gap" && site && insists_on_a_native_half(site)
            outcome = "native-extension"
            detail = ""
          end
          row(name, version, stage, outcome, detail: detail, sha256: sha256, where: site)
        end
      end

      # The ruby the verdict points at, `<repo-relative path>:<line>`, when
      # zeo named one.
      #
      # The detail alone says WHAT zeo refused, not where: `subclassing the
      # built-in type Module` names a construct that appears in dozens of
      # files across a gem's dependency tree, and finding it meant grepping.
      # Repo-relative, because a ledger must not carry one machine's layout.
      def site_of(stderr)
        open = stderr.index("╭─[")
        return nil if open.nil?

        rest = stderr[(open + 3)..]
        close = rest.index("]")
        return nil if close.nil?

        # `path:line:col` -- keep the line, drop the column. A column is
        # precise about a token, and the ledger is read to find a file.
        without_col, _, = rest[0, close].rpartition(":")
        path, _, line = without_col.rpartition(":")
        return nil if path.empty? || line.empty?

        path = path.delete_prefix("#{ROOT}/")
        # A path this scrub did not shorten is outside the repo, and a ledger
        # must not carry one machine's layout.
        path.start_with?("/") ? nil : "#{path}:#{line}"
      end

      # `classify` already re-buckets the two spellings zeo errors on. The
      # third it does NOT error on is a bare `require "redcarpet.so"`, which
      # defers to a runtime LoadError -- so the compile carries on and fails
      # somewhere else entirely, and the row blamed zeo for a gem that cannot
      # run without its compiled half at all.
      def insists_on_a_native_half(site)
        parts = site.split("/")
        return false unless parts[0] == "vendor" && parts[1] == "gems" && parts[2]

        lib = File.join(ROOT, "vendor", "gems", parts[2], "lib")
        return false unless File.directory?(lib)

        Dir.glob("**/*.rb", base: lib).any? do |rel|
          File.foreach(File.join(lib, rel)).any? { |l| requires_a_native_object?(l) }
        rescue SystemCallError
          false
        end
      end

      # A `require`/`require_relative` naming a compiled object file. Comment
      # lines are skipped so a gem that merely MENTIONS the spelling in prose
      # does not count.
      def requires_a_native_object?(line)
        line = line.lstrip
        return false if line.start_with?("#")

        rest = if line.start_with?("require_relative ") then line.delete_prefix("require_relative ")
               elsif line.start_with?("require ") then line.delete_prefix("require ")
               end
        return false if rest.nil?

        rest = rest.strip
        quote = rest[0]
        return false unless quote == %(") || quote == "'"

        feature = rest[1..].to_s.split(quote).first.to_s
        %w[.so .bundle .dll].any? { |e| feature.end_with?(e) }
      end

      # Conventional non-code directories never become roots, so a tests-only
      # archive still reports `no-lib-dir` honestly.
      NON_CODE = %w[spec test tests features benchmark benchmarks bin exe doc docs example
                    examples sample samples vendor tasks rakelib script scripts man data
                    assets].freeze

      # Load-path roots DISCOVERED from the archive when every declared
      # `require_path` is missing. In order: the gem directory itself when
      # bare `.rb` files sit at its top (the archive root IS the load path), a
      # nested `<sub>/lib` (a gem packed one directory too deep), and any
      # code-shaped first-level directory (`gem/`, `ruby/`, `src/`).
      def discovered_roots(dir)
        roots = []
        roots << dir unless Dir.glob("*.rb", base: dir).empty?
        Dir.children(dir).sort.each do |sub|
          next if sub.start_with?(".") || NON_CODE.include?(sub)

          subdir = File.join(dir, sub)
          next unless File.directory?(subdir)

          nested = File.join(subdir, "lib")
          if File.directory?(nested) && ships_ruby?(nested)
            roots << nested
          elsif ships_ruby?(subdir)
            roots << subdir
          end
        end
        roots
      end

      # The honest verdict for a gem whose declared load path does not exist
      # in its archive. Three different facts used to share `no-lib-dir`, and
      # only one of them ever had ruby a compiler could reach.
      def rootless_outcome(dir, meta)
        if !meta["extensions"].empty? || File.directory?(File.join(dir, "ext")) ||
           File.file?(File.join(dir, "extconf.rb"))
          # The gem's code IS its C extension. See docs/EXTENSIONS.md.
          return "ext-only"
        end
        # A gemspec-only gem that exists to name dependencies. `rails` is the
        # canonical one. Nothing to compile, and never a failure.
        return "meta-gem" unless ships_ruby?(dir)

        # The declared load path is empty as packaged, so no ruby can require
        # what this ships either.
        "no-lib-dir"
      end

      def ships_ruby?(dir) = !Dir.glob("**/*.rb", base: dir).first.nil?

      def read_meta(dir)
        JSON.parse(File.read(File.join(dir, ".zeo-probe-spec")))
      rescue StandardError
        default_meta
      end

      # ---- classification ------------------------------------------------

      # A panic, a timeout and a memory kill are each their own outcome rather
      # than folded into `lowering-gap`: a gap is a limit zeo REPORTED, a panic
      # is a bug it did not, a timeout is no verdict at all, and an
      # out-of-memory says what this MACHINE could hold rather than anything
      # about the gem.
      def classify(stderr, dir)
        # The captured stderr is bytes. A diagnostic can carry any byte a gem's
        # source does, so it is scrubbed to valid UTF-8 rather than assumed to
        # be text -- comparing a BINARY string against the box-drawing
        # characters below otherwise raises.
        text = stderr.to_s.dup.force_encoding(Encoding::UTF_8).scrub
        stage = front_end_stage(text)
        if (i = text.lines.index { |l| l.include?("panicked at") })
          msg = text.lines[i + 1].to_s.strip
          msg = text.lines[i].strip if msg.empty?
          return [stage, "compiler-panic", scrub(message_of(msg), dir)]
        end
        return [stage, "out-of-memory", scrub(message_of(text), dir)] if text.include?("ZEO_MEMORY_LIMIT")

        msg = scrub(message_of(text), dir)
        msg = "compile failed" if msg.empty?

        # NATIVE EXTENSION FIRST. zeo says so INSIDE a `cannot load such file`
        # message, so testing the load-failure prefix first swallows every one
        # of them into `missing-dependency` and leaves this arm dead. The two
        # are different verdicts: a missing dependency is a gem the sweep
        # failed to put on the load path, and re-running with it there changes
        # the answer; a native extension is one zeo cannot compile at all.
        # zeo compiles a gem's C from SOURCE now, so what is left in this
        # arm is the two it cannot: a PRECOMPILED extension built against
        # CRuby's ABI, and a build that failed.
        return [stage, "precompiled-extension", ""] if msg.include?("PRECOMPILED extension")
        if (rest = msg.split("building ")[1]) && msg.include?("C extension:")
          return [stage, "extension-build-failed", rest.split("'").first.to_s.strip]
        end
        return [stage, "native-extension", ""] if msg.include?("native (C) extension")
        # The `require_relative "x.so"` spelling: a gem naming its compiled
        # object by PATH, which names no gem for zeo to build. The rescued
        # spelling never errors.
        return [stage, "native-extension", ""] if msg.include?("names no gem to build")

        if (rest = msg.split("cannot load such file -- ")[1])
          feature = rest.split(/\s/).first.to_s.delete("`:.")
          return [stage, "missing-dependency", feature]
        end
        # An ambiguity is the probe's OWN artifact -- the subject gem vendors a
        # file a bundled gem also provides, and only the probe puts both on one
        # load path. Counting it as a lowering gap overstated the backlog by
        # 642 rows.
        return [stage, "ambiguous-require", truncate(msg)] if msg.include?("is ambiguous: found in multiple gems")
        # zeo parses with prism, which IS CRuby's parser, so its parse errors
        # are the ones `ruby -c` gives. Those gems load under no ruby either.
        return [stage, "invalid-ruby", truncate(msg)] if msg.start_with?("parse error: ")

        [stage, "lowering-gap", truncate(msg)]
      end

      # Which front-end pass rejected the gem, read off the diagnostic CODE zeo
      # prints on the first line. The passes fail differently and are fixed
      # differently: a lowering gap is a construct the front end will not
      # translate, an analyze rejection is a definition it will not register,
      # a codegen rejection is a position it will not emit into.
      #
      # A message with no code is a compile that died without a diagnostic.
      # `codegen` -- the last rung -- is the honest answer there: it got as far
      # as anything can without saying otherwise, and the OUTCOME column
      # records that it died.
      def front_end_stage(stderr)
        stderr.each_line do |line|
          case line.strip
          when "zeo::parse" then return "parse"
          when "zeo::lower" then return "lower"
          when "zeo::analyze" then return "analyze"
          when "zeo::codegen" then return "codegen"
          end
        end
        "codegen"
      end

      # The compiler's message, as one line and free of local paths.
      #
      # zeo wraps a diagnostic across `│` continuation lines and prefixes it
      # with the require chain that reached the file, as absolute paths. Both
      # have to go: the wrap so the message survives, and the paths because
      # this text is written to a ledger and must not carry one machine's
      # directory layout.
      def message_of(err)
        boxed = err.lines.map(&:strip).take_while { |l| !l.start_with?("╭") }
                  .select { |l| l.start_with?("×", "│") }
        joined = if boxed.empty?
                   err
                 else
                   boxed.map { |l| l.sub(/\A[×│]+/, "").strip }.join(" ")
                 end
        msg = joined.split(/\s+/).join(" ")
        # Peel `<path>.rb: ` prefixes, one per file in the require chain.
        msg = msg[(msg.index(".rb: ") + 5)..] while msg.include?(".rb: ")
        msg
      end

      # A diagnostic can carry an absolute path ANYWHERE in it -- inside a
      # span, not only as a leading prefix -- so the roots are rewritten away
      # wholesale and any surviving home-rooted token goes with them.
      def scrub(msg, dir)
        msg = msg.gsub("#{dir}/", "").gsub("#{ROOT}/", "")
        return msg unless msg.include?("/Users/") || msg.include?("/home/")

        msg.split(/\s+/).reject { |w| w.include?("/Users/") || w.include?("/home/") }.join(" ")
      end

      def truncate(s, limit = 300) = s.length > limit ? "#{s[0, limit - 1]}…" : s

      # ---- entry point ----------------------------------------------------

      # The feature a gem's users require, resolved against the files the gem
      # actually ships. Every declared root goes on the load path, so a feature
      # found under any of them resolves.
      def entry_point(roots, name)
        roots.each do |lib|
          hit = entry_point_under(lib, name)
          return hit if hit
        end
        nil
      end

      def entry_point_under(lib, name)
        # `net-http` -> `net/http`, `ruby-progressbar` -> `ruby_progressbar`.
        [name.tr("-", "/"), name.tr("-", "_"), name].each do |c|
          return c if File.file?(File.join(lib, "#{c}.rb"))
        end
        # `activerecord` ships `active_record.rb`: the separators differ, so
        # compare with them removed.
        target = squash(name)
        tops = Dir.glob("*.rb", base: lib).map { |f| File.basename(f, ".rb") }.sort
        hit = tops.find { |stem| squash(stem) == target }
        return hit if hit
        # A gem with exactly one top-level file has named its entry point.
        return tops.first if tops.size == 1

        # ONE DIRECTORY DEEPER, for the gems that nest their entry:
        # concurrent-ruby ships `lib/concurrent-ruby/concurrent-ruby.rb`,
        # json_pure ships `lib/json/pure.rb`. Both are still name matches, just
        # against a path rather than a top-level stem, so this stays a
        # resolution rule and not a guess. Either half can carry the name.
        Dir.children(lib).sort.each do |sub|
          subdir = File.join(lib, sub)
          next unless File.directory?(subdir)

          Dir.glob("*.rb", base: subdir).sort.each do |f|
            stem = File.basename(f, ".rb")
            return "#{sub}/#{stem}" if squash(stem) == target || squash("#{sub}/#{stem}") == target
          end
        end
        nil
      end

      def squash(s) = s.delete("-_/").downcase

      # Every top-level `.rb` the roots ship, as features. The tier below the
      # name ladder.
      def top_level_features(roots)
        roots.flat_map { |r| Dir.glob("*.rb", base: r) }.map { |f| File.basename(f, ".rb") }.uniq.sort
      end

      # The features the gem's own REQUIRE GRAPH publishes -- the tier below
      # `top_level_features`, for the pre-convention layout that ships
      # `lib/<dir>/...` with no top-level file at all (`360_services` ships
      # `lib/sorenson/`, and nothing anywhere carries the gem's name).
      #
      # Among the gem's own files, an entry point is a file no sibling
      # requires: in-degree zero in the graph of `require`/`require_relative`
      # edges that resolve to files INSIDE the gem. Of those roots, the ones
      # whose transitive closure reaches the most files are the published
      # surface.
      def require_graph_root_features(roots)
        files = []
        roots.each { |r| collect_rb_features(r, r, 0, files) }
        files.sort_by!(&:first)
        files.uniq!(&:first)
        return [] if files.empty?

        index = files.each_with_index.to_h { |(f, _), i| [f, i] }
        out_edges = Array.new(files.size) { [] }
        in_degree = Array.new(files.size, 0)
        files.each_with_index do |(feature, path), i|
          src = begin
            File.read(path)
          rescue SystemCallError
            next
          end
          literal_requires(src).each do |relative, target|
            f = if relative
                  dir = feature.include?("/") ? feature[0...feature.rindex("/")] : ""
                  normalize_feature("#{dir}/#{target}") or next
                else
                  target
                end
            # An edge only when the required feature is one of the gem's OWN
            # files -- a dependency's feature resolves elsewhere and says
            # nothing about which of these files is the entry.
            j = index[f]
            next if j.nil? || j == i || out_edges[i].include?(j)

            out_edges[i] << j
            in_degree[j] += 1
          end
        end
        root_ixs = (0...files.size).select { |i| in_degree[i].zero? }
        return [] if root_ixs.empty?

        covs = root_ixs.map { |i| coverage(out_edges, i) }
        max = covs.max
        root_ixs.zip(covs).select { |_, c| c == max }.map { |i, _| files[i].first }
      end

      def coverage(out_edges, start)
        seen = {}
        stack = [start]
        n = 0
        until stack.empty?
          i = stack.pop
          next if seen[i]

          seen[i] = true
          n += 1
          stack.concat(out_edges[i])
        end
        n
      end

      # Every `.rb` under `dir` as a `[feature, path]` pair, the feature
      # root-relative with the extension dropped. Symlinked directories are
      # NOT followed -- a gem archive can carry a symlink cycle, and one spun
      # the probe forever on the last gem of the first entry-point sweep. The
      # depth cap is the backstop for a cycle spelled without symlinks.
      def collect_rb_features(root, dir, depth, out)
        return if depth > 32

        Dir.children(dir).each do |base|
          path = File.join(dir, base)
          # `lstat` reads the entry itself and never follows a symlink.
          st = begin
            File.lstat(path)
          rescue SystemCallError
            next
          end
          if st.directory?
            collect_rb_features(root, path, depth + 1, out)
          elsif st.file? && base.end_with?(".rb")
            out << [path.delete_prefix("#{root}/").delete_suffix(".rb"), path]
          end
        end
      rescue SystemCallError
        nil
      end

      # The `require "x"` / `require_relative "y"` targets a source spells as
      # a single literal string -- the only forms that name a file this side
      # of execution. A computed or interpolated argument contributes no edge.
      def literal_requires(src)
        out = []
        src.each_line do |line|
          line = line.lstrip
          relative = line.start_with?("require_relative")
          rest = if relative
                   line.delete_prefix("require_relative")
                 elsif line.start_with?("require")
                   line.delete_prefix("require")
                 end
          next if rest.nil?

          rest = rest.sub(/\A[(\s]+/, "")
          quote = rest[0]
          next unless quote == %(") || quote == "'"

          body = rest[1..].to_s
          stop = body.index(quote)
          next if stop.nil?

          target = body[0, stop]
          next if target.empty? || target.include?('#{')

          out << [relative, target.delete_suffix(".rb")]
        end
        out
      end

      # Resolves `.` and `..` segments textually; nil when `..` escapes the
      # root, meaning the file lives outside the load path.
      def normalize_feature(feature)
        parts = []
        feature.split("/").each do |seg|
          case seg
          when "", "." then next
          when ".." then return nil if parts.pop.nil?
          else parts << seg
          end
        end
        parts.join("/")
      end

      # A gem with no library at all can still publish executables. A
      # ruby-shebang script in `bin/` or `exe/` is its surface.
      def ruby_executables(dir)
        %w[bin exe].flat_map do |b|
          sub = File.join(dir, b)
          next [] unless File.directory?(sub)

          Dir.children(sub).sort.map { |f| File.join(sub, f) }.select do |p|
            next false unless File.file?(p)

            first = begin
              File.open(p, "rb") { |f| f.read(128).to_s }
            rescue SystemCallError
              ""
            end.lines.first.to_s
            first.start_with?("#!") && first.include?("ruby")
          end
        end.sort
      end

      # zeo parses gemspecs statically, so the computed `s.version` most real
      # gems use (`spec.version = Colorator::VERSION`) is rejected, and zeo
      # also checks a gemspec's name against its directory name.
      def write_stub_gemspec(dir, name, version, require_paths)
        Dir.glob(File.join(dir, "*.gemspec")).each { |p| FileUtils.rm_f(p) }
        paths = require_paths.map { |p| "#{p.inspect}.freeze" }.join(", ")
        File.write(File.join(dir, "#{name}.gemspec"), <<~SPEC)
          Gem::Specification.new do |s|
            s.name = #{name.inspect}.freeze
            s.version = #{version.inspect}.freeze
            s.require_paths = [#{paths}]
          end
        SPEC
      end

      # ---- the ledger ------------------------------------------------------

      def row(gem, version, stage, outcome, rust_bytes: nil, detail: nil, where: nil, sha256: nil)
        { "gem" => gem, "version" => version.to_s, "stage" => stage, "outcome" => outcome,
          "rust_bytes" => rust_bytes&.to_s || "", "binary_bytes" => "",
          "detail" => (detail || "").tr("\t\n", "  "), "where" => where || "",
          "sha256" => sha256 || "" }
      end

      def ledger_path = File.join(ROOT, LEDGER)

      def read_ledger
        return {} unless File.exist?(ledger_path)

        _comments, rows = Tsv.read(ledger_path)
        rows.shift if rows.first == COLUMNS
        rows.each_with_object({}) do |cells, h|
          next if cells.empty? || cells.first.to_s.empty?

          h[cells.first] = COLUMNS.zip(cells).to_h { |k, v| [k, v.to_s] }
        end
      end

      # Written whole and sorted, so an interrupted sweep leaves a file that
      # still reads, and verdicts arriving out of order do not reorder it.
      def write_ledger(ledger)
        out = +"#{Tsv.row(*COLUMNS)}\n"
        ledger.keys.sort.each { |g| out << Tsv.row(*COLUMNS.map { |c| ledger[g][c] }) << "\n" }
        File.write(ledger_path, out)
        warn "gem-probe: wrote #{LEDGER} (#{ledger.size} rows)"
      end

      def summarize(rows)
        by = rows.group_by { |r| r["outcome"] }
        ok = by["ok"]&.size || 0
        puts "\n#{ok}/#{rows.size} compiled"
        by.sort_by { |k, v| [-v.size, k] }.each do |outcome, list|
          next if outcome == "ok"

          puts format("  %-22s %4d  %s", outcome, list.size,
                      list.first["detail"].to_s[0, 60])
        end
      end

      # ---- the registry ----------------------------------------------------

      def get_json(url) = JSON.parse(get_bytes(url))

      # `Connection: close` on purpose: a keep-alive HTTPS request never
      # returns under a zeo-compiled build of this tool
      # (`tests/gaps/an_https_keepalive_request_completes.rb`). Under `ruby` it
      # only costs a connection per request, which a sweep already dwarfs.
      def get_bytes(url, redirects = 5)
        raise Error, "too many redirects for #{url}" if redirects.zero?

        uri = URI.parse(url)
        req = Net::HTTP::Get.new(uri)
        req["Connection"] = "close"
        req["User-Agent"] = "zeo-dev gem-probe"
        res = Net::HTTP.start(uri.host, uri.port, use_ssl: uri.scheme == "https",
                                                  open_timeout: 30, read_timeout: 120) do |http|
          http.request(req)
        end
        case res
        when Net::HTTPSuccess then res.body
        when Net::HTTPRedirection then get_bytes(res["location"], redirects - 1)
        else raise Error, "#{url}: HTTP #{res.code}"
        end
      rescue SocketError, Errno::ECONNREFUSED, Net::OpenTimeout, Net::ReadTimeout => e
        raise Error, "#{url}: #{e.class}"
      end
    end
  end
end

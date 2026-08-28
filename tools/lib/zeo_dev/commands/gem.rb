# frozen_string_literal: true

require "fileutils"
require "tmpdir"

module ZeoDev
  module Commands
    # Vendor pure-Ruby stdlib gems into `gems/` from their upstream git repos.
    #
    #   gem add <owner/repo> --tag <t> [--name <n>] [--subdir <d>]
    #   gem sync [<name>] [--check] [--check-oracle]
    #   gem update <name> [--tag <t>]
    #   gem outdated [<name>]
    #   gem lock [--check]
    #
    # `rev` is trusted as written once resolved: the tag is the source of
    # truth and there is no cross-check against the local install.
    # `--check-oracle` is an optional extra.
    class Gem < Cli
      SUBCOMMANDS = %w[add sync update outdated lock ambient].freeze

      # Where the ambient list is written. Committed, because the golden
      # harness reads it from Rust and must not run ruby to learn it.
      AMBIENT = "gems/AMBIENT.txt"

      # Two gems whose `lib/` the oracle must never be given. Zeo vendors its
      # own copies, but ruby loads its own rubygems before the first line of
      # any program runs; putting zeo's ahead of it replaces the oracle's
      # loader with an untested one.
      NEVER_AMBIENT = %w[rubygems bundler].freeze

      # Reading a version off the install is easy to get wrong: `gem install`
      # drops newer copies into the same store and `require` activates the
      # newest, which is how reline 0.7.0 and webrick 1.9.2 ended up in two
      # goldens as if ruby had shipped them.

      def self.summary = "manage the vendored upstream gems"

      def self.banner = <<~TEXT
        usage: zeo-dev gem <subcommand> [options]

        subcommands:
          add <owner/repo> --tag <t> [--name <n>] [--subdir <d>]
          sync [<name>] [--check] [--check-oracle]
          update <name> [--tag <t>]
          outdated [<name>]
          lock [--check]
          ambient [--check]
      TEXT

      def defaults = { tag: nil, name: nil, subdir: nil, check: false, check_oracle: false }

      def options(o)
        o.on("--tag TAG", "--ref TAG", "--branch TAG", "the git ref to pin") { |v| opts[:tag] = v }
        o.on("--name NAME", "the vendored name, when it is not the repo's") { |v| opts[:name] = v }
        o.on("--subdir DIR", "the directory inside the checkout holding lib/") { |v| opts[:subdir] = v }
        o.on("--check", "verify without writing; nonzero on drift") { opts[:check] = true }
        o.on("--check-oracle", "also report how the vendored tree compares to the install") do
          opts[:check_oracle] = true
        end
      end

      def run
        sub = args.shift
        raise Error, self.class.banner unless SUBCOMMANDS.include?(sub)

        send("cmd_#{sub}")
      end

      private

      def manifest = @manifest ||= Manifest.load

      def group = :gems

      def gems_dir(entry) = File.join(ROOT, "gems", entry.name)

      # `gem add`: append a manifest entry from the CLI arguments -- never by
      # parsing ruby -- and vendor it.
      def cmd_add
        owner_repo = args.shift or raise Error, "gem add needs <owner/repo>, e.g. ruby/fileutils"
        tag = opts[:tag] or raise Error, "gem add needs --tag <tag>, e.g. --tag v1.8.0"
        # The repo's own name, unless a repo shipping several gems named one.
        name = opts[:name] || owner_repo.split("/").last
        raise Error, "cannot derive a gem name from #{owner_repo.inspect}" if name.nil? || name.empty?
        if manifest.find(name)
          raise Error, "#{name} is already in upstream.rb -- use " \
                       "`gem update #{name} --tag <t>` to change it"
        end

        append_source(name, owner_repo, tag, opts[:subdir])
        @manifest = nil
        puts "added #{name} to #{Manifest::SOURCE}"
        sync_entries([manifest.find(name)], write: true)
      end

      # `gem sync [<name>]`: vendor every gem (or one) from its pinned rev,
      # resolving the tag on first sync. `--check` verifies the committed
      # `lib/` matches upstream without writing.
      def cmd_sync
        only = args.shift
        entries = manifest.group(group)
        entries = entries.select { |e| e.name == only } if only
        raise Error, "#{only} is not in #{Manifest::SOURCE}" if entries.empty?

        status = sync_entries(entries, write: !opts[:check])
        entries.each { |e| check_against_oracle(e) } if opts[:check_oracle]
        status
      end

      # `gem update <name> [--tag <t>]`: bump the tag and re-resolve the rev,
      # then re-vendor.
      def cmd_update
        name = args.shift or raise Error, "gem update needs <name>"
        entry = manifest.find(name, group: group) or raise Error, "#{name} is not in #{Manifest::SOURCE}"
        entry.tag = opts[:tag] if opts[:tag]
        entry.rev = nil # force re-resolution of the (possibly new) tag
        sync_entries([entry], write: true)
      end

      # Rewrites `upstream.lock` from `upstream.rb`. `--check` fails on drift,
      # which is what CI runs -- the lock is the only file anything outside
      # `tools/` reads.
      def cmd_lock
        json = manifest.lock_json
        return Tsv.check(manifest.lock_path, json, label: Manifest::LOCK) if opts[:check]

        manifest.write_lock!
        puts "wrote #{Manifest::LOCK} (#{manifest.entries.size} entries)"
        0
      end

      # `gem ambient [--check]`: rewrite `gems/AMBIENT.txt`, the gems the
      # ruby oracle has to be told about.
      #
      # The oracle runs with an EMPTY gem store, so it sees ruby's default
      # gems and nothing else -- no gem anybody happened to `gem install` on
      # the machine can reach a golden. That was not hypothetical: reline
      # 0.7.0 and webrick 1.9.2 sat in this machine's store, and two goldens
      # recorded them as if ruby 4.0.6 shipped them.
      #
      # An empty store also hides the BUNDLED gems, which are real libraries
      # ruby ships, so each of those is handed back as one `-I` root. The
      # result is one library set for both engines: a difference between them
      # is then zeo's, which is the only kind a golden should record.
      def cmd_ambient
        content = Tsv.render(ambient_rows, comments: AMBIENT_HEADER)
        return Tsv.check(File.join(ROOT, AMBIENT), content, label: AMBIENT) if opts[:check]

        Tsv.write(File.join(ROOT, AMBIENT), content)
        puts "wrote #{AMBIENT} (#{ambient_rows.size} gems)"
        0
      end

      AMBIENT_HEADER = [
        "# The `-I` roots the ruby oracle runs with, one gem per line.",
        "# Generated by `tools/zeo-dev gem ambient`; `--check` gates it in CI.",
        "#",
        "# The oracle runs with an EMPTY gem store so no gem anybody happened to",
        "# `gem install` can reach a golden. That also hides ruby's BUNDLED gems,",
        "# which are real libraries it ships, so each is named here with the tree",
        "# to read it from:",
        "#",
        "#   zeo         zeo's own `gems/<name>/lib` -- a faithful upstream copy,",
        "#               held byte-identical by `gem sync --check`.",
        "#   ruby:<ver>  ruby's own `<gemdir>/gems/<name>-<ver>/lib`, for a gem",
        "#               whose Ruby half zeo AUTHORED over a native half in",
        "#               zeo-rt. Ruby cannot run zeo's copy.",
        "#",
        "# Default gems are absent: ruby finds those with no store at all.",
      ].freeze

      # One row per gem the oracle would otherwise lose, and where to read it.
      #
      # The base set is what ruby SHIPPED -- its bundled gems, at its own
      # versions. A row survives only if zeo can answer that require too,
      # either from a faithful vendored copy or from a native half in zeo-rt;
      # anything else is a gem somebody installed on this machine, and giving
      # it to the oracle is the pollution this whole mechanism removes.
      def ambient_rows
        @ambient_rows ||= shipped_gem_versions.sort.filter_map do |name, version|
          spec = File.join(ROOT, "gems", name, "#{name}.gemspec")
          # A faithful vendored copy is preferred: it is the same tree zeo
          # compiles, so the two engines cannot disagree about a version.
          # `gem sync --check` holds it byte-identical to upstream.
          if File.file?(spec) && File.read(spec)[0, 200].include?("Vendored from")
            [name, "zeo"]
          elsif zeo_answers?(name)
            # Zeo has its own half (`bigdecimal`, `nkf`, ...) and ruby cannot
            # run it, so the oracle reads ruby's own copy instead.
            [name, "ruby:#{version}"]
          end
        end
      end

      # Whether zeo can `require` this feature at all. The probe is one
      # compile per candidate and runs only when this table is regenerated.
      def zeo_answers?(name)
        bin = Ruby.zeo
        return false unless File.executable?(bin)

        Exec.run([bin, "-e", "require #{name.inspect}"], chdir: ROOT).success?
      end

      # The gems the oracle ruby ships in its own stdlib, read from the
      # `specifications/default/` directory rubygems keeps them in.
      def default_gem_names
        dir = gemdir && File.join(gemdir, "specifications", "default")
        return [] unless dir && File.directory?(dir)

        Dir.children(dir).filter_map { |f| f[/\A(.+)-[\d.]+\.gemspec\z/, 1] }
      end

      # The BUNDLED gems ruby installed, at the versions it installed.
      #
      # Nothing on disk marks a shipped gem apart from one somebody installed
      # later, and reading the newest is exactly the mistake that put reline
      # 0.7.0 in two goldens. The one honest signal is the clock: ruby writes
      # every gemspec in one batch, and `specifications/default/` -- a
      # directory ruby owns outright -- dates that batch.
      def shipped_gem_versions
        dir = gemdir && File.join(gemdir, "specifications")
        return {} unless dir && File.directory?(dir)

        installed_at = Dir.glob(File.join(dir, "default", "*.gemspec"))
                          .map { |f| File.mtime(f).strftime("%F %H:%M") }.min
        return {} if installed_at.nil?

        Dir.glob(File.join(dir, "*.gemspec")).each_with_object({}) do |f, found|
          next unless File.mtime(f).strftime("%F %H:%M") == installed_at
          next unless (m = File.basename(f).match(/\A(.+)-([\d.]+)\.gemspec\z/))
          next if NEVER_AMBIENT.include?(m[1])

          found[m[1]] = m[2]
        end
      end

      # The core loop: fetch and materialize each selected entry.
      def sync_entries(entries, write:)
        dirty = false
        drift = []
        entries.each do |entry|
          if entry.rev.nil?
            entry.rev = Vendor.resolve_tag(entry.url, entry.tag)
            dirty = true
          end
          checkout = Vendor.fetch_checkout(entry, entry.rev)
          if write
            materialize(checkout, entry)
            puts "vendored #{entry.name} @ #{entry.tag} (#{entry.short_rev})"
          else
            tmp = File.join(Dir.tmpdir, "zeo-gemcheck-#{entry.name}")
            FileUtils.rm_rf(tmp)
            materialize(checkout, entry, dest: tmp)
            if Vendor.dirs_equal?(File.join(tmp, "lib"), File.join(gems_dir(entry), "lib"))
              puts "ok #{entry.name} @ #{entry.tag} (#{entry.short_rev})"
            else
              drift << entry.name
            end
            FileUtils.rm_rf(tmp)
          end
        end
        rewrite_source! if dirty && write
        manifest.write_lock! if write
        unless drift.empty?
          raise Error, "vendored lib/ drifted from upstream for: #{drift.join(", ")} " \
                       "(run `tools/zeo-dev gem sync` to re-vendor)"
        end
        0
      end

      def materialize(checkout, entry, dest: nil)
        Vendor.vendor_lib(checkout, dest || gems_dir(entry), entry)
      end

      # `gem outdated`: one row per entry -- the current pin, the version the
      # ORACLE RUBY installs, and the newest upstream tag.
      #
      # The oracle column is the one that matters. zeo's conformance target is
      # the ruby in `mise.toml`, and every golden is blessed by running it, so
      # a gem vendored AHEAD of what that ruby ships manufactures divergences
      # that are not bugs. The upstream column says how far the ecosystem has
      # moved past the oracle.
      #
      # Read-only. Feed the oracle column back in with
      # `gem update <name> --tag v<version>`.
      def cmd_outdated
        only = args.shift
        installed = installed_gem_versions
        entries = manifest.group(group)
        entries = entries.select { |e| e.name == only } if only

        puts format("%-14s %-12s %-12s %-12s action", "gem", "pinned", "oracle", "upstream")
        behind = []
        entries.each do |entry|
          oracle = installed[entry.name]
          upstream = newest_tag(entry.url)
          action = if oracle.nil?
                     # bundler and rubygems live outside the store's
                     # versioned layout, so there is nothing to match.
                     "(not in the oracle install)"
                   elsif oracle == entry.version
                     "-"
                   else
                     behind << [entry.name, oracle]
                     "gem update #{entry.name} --tag v#{oracle}"
                   end
          puts format("%-14s %-12s %-12s %-12s %s", entry.name, entry.version,
                      oracle || "?", upstream || "?", action)
        end
        if behind.empty?
          puts "\nEvery pinned gem matches the oracle install."
        else
          puts "\n#{behind.size} gem(s) differ from the oracle:"
          behind.each { |n, v| puts "  tools/zeo-dev gem update #{n} --tag v#{v}" }
        end
        0
      end

      # Every gem version the oracle ruby has installed, from
      # `<gemdir>/gems/<name>-<version>/`. Highest wins when several are
      # installed side by side.
      def installed_gem_versions
        dir = gemdir
        return {} if dir.nil?

        found = {}
        Dir.children(File.join(dir, "gems")).each do |child|
          # `<name>-<version>`, where the name itself may contain dashes
          # (`net-http-0.9.1`): split at the LAST dash starting a digit.
          i = child.rindex(/-(?=\d)/)
          next if i.nil?

          name = child[0...i]
          version = child[(i + 1)..]
          found[name] = version if found[name].nil? || version_lt(found[name], version)
        end
        found
      rescue SystemCallError
        {}
      end

      # The `mise.toml`-pinned gem, not whatever is on PATH -- the point is to
      # match what the CONFORMANCE ORACLE runs.
      def gemdir
        @gemdir ||= begin
          bin = `mise which gem 2>/dev/null`.strip
          bin = "gem" if bin.empty?
          out = `#{bin} env gemdir 2>/dev/null`.strip
          out.empty? ? nil : out
        end
      end

      # The newest `vX.Y.Z` tag on a remote, by numeric component order.
      # `nil` when the remote is unreachable -- this column is informational,
      # so a network hiccup must not fail the command.
      def newest_tag(url)
        out = Vendor.git(["ls-remote", "--tags", "--refs", url])
        newest = nil
        out.each_line(chomp: true) do |line|
          _, refname = line.split("\t", 2)
          next if refname.nil?

          version = refname.delete_prefix("refs/tags/").delete_prefix("v")
          # Releases only: skip `1.2.3.pre1`, `v1.2.3-rc` and the like.
          next unless version.match?(/\A\d+(\.\d+)*\z/)

          newest = version if newest.nil? || version_lt(newest, version)
        end
        newest
      rescue Error
        nil
      end

      # `a < b` over dot-separated numeric components, shorter-is-lower on a
      # common prefix (`1.2` < `1.2.1`).
      def version_lt(a, b)
        x = a.split(".").map(&:to_i)
        y = b.split(".").map(&:to_i)
        [x.size, y.size].max.times do |i|
          next if x.fetch(i, 0) == y.fetch(i, 0)

          return x.fetch(i, 0) < y.fetch(i, 0)
        end
        false
      end

      # INFORMATIONAL, not a gate. A gem's upstream repo and ruby-core's
      # bundled copy are not the same tree -- ruby-core patches several
      # default gems in place between releases -- so this is for spotting a
      # vendored copy that has fallen a whole VERSION behind. The real gate is
      # `--check`.
      def check_against_oracle(entry)
        vendored = File.join(gems_dir(entry), "lib")
        gems_root = gemdir && File.join(gemdir, "gems", "#{entry.name}-#{entry.version}", "lib")
        installed, report_extras = if gems_root && File.directory?(gems_root)
                                     [gems_root, true]
                                   else
                                     [default_gem_dir(vendored), false]
                                   end
        if installed.nil?
          puts "skip oracle-check #{entry.name}: not installed at #{gems_root}"
          return
        end
        ours = Vendor.list_files(vendored)
        differing = ours.reject do |rel|
          other = File.join(installed, rel)
          File.file?(other) && File.binread(other) == File.binread(File.join(vendored, rel))
        end
        if !differing.empty?
          puts "oracle-differs #{entry.name} @ #{entry.version}: #{differing.join(", ")} (vs #{installed})"
          return
        end
        extra = Vendor.list_files(installed) - ours
        if extra.empty?
          puts "oracle-ok #{entry.name} @ #{entry.version}"
        elsif report_extras
          # The install may ship files upstream does not -- racc's
          # `parser-text.rb` is generated by its build step. That is not
          # drift; the extras are named so a genuinely new one gets noticed.
          puts "oracle-ok #{entry.name} @ #{entry.version} (install also ships: #{extra.join(", ")})"
        else
          puts "oracle-ok #{entry.name} @ #{entry.version} (default gem)"
        end
      end

      # A DEFAULT gem is not unpacked under `gems/` at all -- its files sit
      # directly in ruby's own stdlib dir. "Any vendored file lands there" is
      # enough to say this IS the install; requiring all of them would skip a
      # gem that ships a file ruby-core drops (open3's `jruby_windows.rb`),
      # which is exactly the sort of thing worth reporting.
      def default_gem_dir(vendored)
        res = Exec.run(Ruby.oracle_argv("-rrbconfig", "-e", "print RbConfig::CONFIG['rubylibdir']"),
                       env: Ruby.oracle_env, capture_stdout: true, chdir: ROOT)
        out = res.stdout.to_s.strip
        return nil if out.empty? || !File.directory?(out)

        Vendor.list_files(vendored).any? { |rel| File.file?(File.join(out, rel)) } ? out : nil
      end

      # `upstream.rb` is hand-editable ruby, so a resolved rev is spliced into
      # the entry it belongs to rather than the file being regenerated -- a
      # regeneration would drop every comment someone wrote beside an entry.
      def rewrite_source!
        path = File.join(ROOT, Manifest::SOURCE)
        text = File.read(path)
        manifest.entries.each do |e|
          next if e.rev.nil?

          block = entry_block(text, e)
          next if block.nil? || block.include?("rev:")

          updated = block.sub(/(\n\s*tag: "[^"]*",?)/) { "#{::Regexp.last_match(1).chomp(",")},\n  rev: #{e.rev.inspect}" }
          text = text.sub(block, updated)
        end
        File.write(path, text)
      end

      def entry_block(text, entry)
        verb = entry.group == :gemtests ? "gemtest" : "gem"
        text[/^#{verb} #{Regexp.escape(entry.name.inspect)},\n(?:  \S.*\n)+/]
      end

      def append_source(name, owner_repo, tag, subdir)
        path = File.join(ROOT, Manifest::SOURCE)
        block = +"gem #{name.inspect},\n  github: #{owner_repo.inspect},\n  tag: #{tag.inspect}"
        block << ",\n  subdir: #{subdir.inspect}" if subdir
        File.write(path, "#{File.read(path).rstrip}\n#{block}\n")
      end
    end
  end
end

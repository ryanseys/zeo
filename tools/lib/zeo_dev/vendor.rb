# frozen_string_literal: true

require "fileutils"
require "tmpdir"

module ZeoDev
  # Fetching an upstream tree and materializing it.
  #
  # The model is VENDOR-ON-FETCH: a git URL plus a tag is the source, and the
  # committed `gems/<name>/` is the storage the compiler reads. A fresh clone
  # of zeo builds offline; only adding or updating a vendored tree runs the
  # network. Vendoring needs only `git` -- ruby and bundler are not required.
  module Vendor
    # Licenses carried per `gems/UPSTREAM.md` policy.
    LICENSES = %w[COPYING BSDL LICENSE LICENSE.txt LICENSE.md MIT-LICENSE].freeze

    module_function

    def git(args, dir: nil)
      res = Exec.run(["git", *args], chdir: dir, capture_stdout: true)
      raise Error, "git #{args.join(" ")} failed: #{res.stderr.strip}" unless res.success?

      res.stdout
    end

    def cache_dir
      base = ENV["XDG_CACHE_HOME"] || (ENV["HOME"] && File.join(ENV["HOME"], ".cache")) || Dir.tmpdir
      File.join(base, "zeo", "gems")
    end

    # Resolve `refs/tags/<tag>` to a commit SHA, preferring the peeled
    # (`^{}`) object so an annotated tag yields the commit, not the tag.
    def resolve_tag(url, tag)
      out = git(["ls-remote", url, "refs/tags/#{tag}^{}", "refs/tags/#{tag}"])
      peeled = direct = nil
      out.each_line(chomp: true) do |line|
        sha, refname = line.split("\t", 2)
        next if refname.nil?

        refname.end_with?("^{}") ? peeled = sha : direct = sha
      end
      peeled || direct || raise(Error, "tag #{tag} not found in #{url}")
    end

    # Fetch `rev` (via its tag) into a SHA-stamped cache dir. A stamped hit
    # short-circuits the network entirely.
    def fetch_checkout(entry, rev)
      cache = File.join(cache_dir, "#{entry.name}-#{rev}")
      stamp = File.join(cache, ".zeo-vendor-stamp")
      return cache if File.file?(stamp)

      FileUtils.rm_rf(cache)
      FileUtils.mkdir_p(cache)
      git(["init", "--quiet"], dir: cache)
      git(["fetch", "--depth", "1", entry.url, "refs/tags/#{entry.tag}"], dir: cache)
      git(["checkout", "--quiet", "--detach", "FETCH_HEAD"], dir: cache)
      head = git(["rev-parse", "HEAD"], dir: cache).strip
      if head != rev
        raise Error, "#{entry.name}: tag #{entry.tag} resolved to #{rev} but the fetched " \
                     "commit is #{head} (upstream tag moved -- run `gem update #{entry.name}`)"
      end
      # Strip the git metadata: the cache holds a plain, read-only tree.
      FileUtils.rm_rf(File.join(cache, ".git"))
      File.write(stamp, rev)
      cache
    end

    def source_root(checkout, entry) = entry.subdir ? File.join(checkout, entry.subdir) : checkout

    # Copy the checkout's `lib/` (plus any license) into `dest` and write a
    # stub gemspec. Only `lib/` is upstream source the drift check guards; the
    # gemspec is deterministic tool output.
    def vendor_lib(checkout, dest, entry)
      src_root = source_root(checkout, entry)
      src_lib = File.join(src_root, "lib")
      raise Error, "#{entry.name}: upstream has no lib/ directory at #{src_lib}" unless File.directory?(src_lib)

      FileUtils.rm_rf(File.join(dest, "lib"))
      FileUtils.mkdir_p(dest)
      copy_tree(src_lib, File.join(dest, "lib"))
      LICENSES.each do |lic|
        # A sub-gem carries its own license when it has one, else the repo's.
        from = [File.join(src_root, lic), File.join(checkout, lic)].find { |p| File.file?(p) }
        FileUtils.cp(from, File.join(dest, lic)) if from
      end
      write_gemspec(dest, entry, vendored: true)
    end

    # The FULL checkout: `lib/`, `test/`, fixtures and all.
    def vendor_tree(checkout, dest, entry)
      FileUtils.rm_rf(dest)
      copy_tree(source_root(checkout, entry), dest)
      write_gemspec(dest, entry, vendored: false)
    end

    # The loader requires `spec.name == directory name`, and upstream
    # gemspecs are often non-literal (`s.version = X::VERSION`), so the stub
    # replaces whatever the checkout carried.
    def write_gemspec(dest, entry, vendored:)
      Dir.glob(File.join(dest, "*.gemspec")).each { |p| FileUtils.rm_f(p) }
      provenance = if vendored
                     "# Vendored from #{entry.url} @ #{entry.tag} (#{entry.rev}); " \
                       "managed by `zeo-dev gem`.\n# Do not edit by hand -- see upstream.rb.\n"
                   else
                     "# Test-tree vendor of #{entry.url} @ #{entry.tag}; " \
                       "managed by `zeo-dev gemtests`.\n"
                   end
      File.write(File.join(dest, "#{entry.name}.gemspec"), <<~SPEC)
        #{provenance}Gem::Specification.new do |s|
          s.name = #{entry.name.inspect}
          s.version = #{entry.version.inspect}
          s.require_paths = ["lib"]
        end
      SPEC
    end

    def copy_tree(src, dest)
      FileUtils.mkdir_p(dest)
      Dir.children(src).sort.each do |child|
        from = File.join(src, child)
        to = File.join(dest, child)
        File.directory?(from) ? copy_tree(from, to) : FileUtils.cp(from, to)
      end
    end

    # Relative paths of every file under `dir`, sorted. Empty when absent.
    def list_files(dir)
      return [] unless File.directory?(dir)

      # FNM_DOTMATCH so a dotfile is vendored like any other; `.` and `..`
      # come back with it and are dropped by name.
      Dir.glob("**/*", File::FNM_DOTMATCH, base: dir)
         .reject { |r| r == "." || r.end_with?("/.", "/..") || File.directory?(File.join(dir, r)) }
         .sort
    end

    # Recursive byte-for-byte comparison: same file set, same contents.
    def dirs_equal?(a, b)
      files = list_files(a)
      return false if files != list_files(b)

      files.all? { |rel| File.binread(File.join(a, rel)) == File.binread(File.join(b, rel)) }
    end
  end
end

# frozen_string_literal: true

module ZeoDev
  # The libraries the compiler ships, flattened to the one `gems/` directory
  # every install channel carries.
  #
  # The dev tree keeps them in two places -- zeo's own Ruby halves beside the
  # Rust that implements them, the vendored upstream copies in `gems/`. This is
  # where the two become one, so `dist` and `stage-publish` cannot disagree
  # about what ships.
  module Payload
    EXT = "crates/zeo-rt/src/ext"
    GEMS = "gems"

    # `["json/lib/json.rb", "/abs/path/..."]` pairs, sorted, for every library.
    # Everything in a library directory ships -- gemspec, licence text, the
    # `lib/` tree -- except the Rust a colocated half sits beside.
    def self.files
      library_dirs.flat_map do |name, dir|
        shipped(dir).map { |rel| ["#{name}/#{rel}", File.join(dir, rel)] }
      end.sort
    end

    # zeo's halves first, so a name they both carry resolves to zeo's -- the
    # same precedence the loader applies (`bundled_gems_dirs`).
    def self.library_dirs
      dirs = {}
      each_library(File.join(ROOT, EXT)) { |name, path| dirs[name] = path }
      each_library(File.join(ROOT, GEMS)) { |name, path| dirs[name] ||= path }
      dirs.sort.to_h
    end

    # A library is a directory with a `lib/`. Under `ext/` that skips every
    # extension whose Rust needs no Ruby half.
    def self.each_library(dir)
      return unless File.directory?(dir)

      Dir.children(dir).sort.each do |name|
        path = File.join(dir, name)
        yield name, path if File.directory?(File.join(path, "lib"))
      end
    end
    private_class_method :each_library

    def self.shipped(dir)
      Dir.glob("**/*", base: dir)
         .select { |rel| File.file?(File.join(dir, rel)) }
         .reject { |rel| rel.end_with?(".rs") || File.basename(rel) == ".DS_Store" }
         .sort
    end
    private_class_method :shipped
  end
end

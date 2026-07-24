# The P-B core classes: File, Dir, Time, Process, ENV.
#
# Everything here is zone- and machine-independent on purpose: Time uses a
# fixed epoch read in UTC, paths are built under a scratch directory this
# script creates, and nothing prints a real pid/clock/environment value --
# only facts about them. That is what lets the output be diffed against real
# ruby at all.

# --- File: the pure-path family (string work, never touches the disk) -------
puts File.basename("/home/user/notes.md")
puts File.basename("/home/user/notes.md", ".md")
puts File.basename("/home/user/notes.md", ".*")
puts File.basename("/a/b/")
puts File.dirname("/home/user/notes.md")
puts File.dirname("solo")
puts File.dirname("/x")
p File.extname("archive.tar.gz")
p File.extname(".bashrc")      # a LEADING dot is a name, not an extension
p File.extname("trailing.")    # ...but a TRAILING one is an extension
p File.split("/a/b/c.rb")
puts File.join("a", "b", "c")
puts File.join("a/", "/b")     # the separator at the seam collapses
p File.absolute_path?("/abs")
p File.absolute_path?("rel")

# --- Time: a fixed instant, read in UTC ------------------------------------
t = Time.at(1700000000).getutc
puts t.to_s
puts t.inspect
p [t.year, t.month, t.day, t.hour, t.min, t.sec]
p [t.wday, t.yday]
p t.to_i
p t.utc?
p t.zone
p t.utc_offset
p t.monday?, t.tuesday?

# strftime -- the directive table, including Ruby's own padding flags.
puts t.strftime("%Y-%m-%d %H:%M:%S")
puts t.strftime("%F %T")
puts t.strftime("%a %A %b %B")
puts t.strftime("%j %u %w %p %I")
puts t.strftime("%z %Z")
puts t.strftime("100%% literal")
jan = Time.at(1704067200).getutc  # 2024-01-01, a single-digit month
puts jan.strftime("%m|%-m|%_m")

# Arithmetic: `t + n` is a Time, `t - other_time` is a Float of seconds.
p (t + 60).to_i
p (t - 60).to_i
p (Time.at(100) - Time.at(40))
p (Time.at(100) - Time.at(40)).class

# A Float epoch is kept EXACTLY -- `subsec` answers the double's true
# fraction, and `nsec` is the truncated view of it.
p Time.at(0.5).subsec
p Time.at(10.8).subsec
p Time.at(10.8).nsec
p (Time.at(10.8) - 0.9).nsec
p Time.at(1.25).to_f
p Time.at(-0.5).to_i           # floored: second -1 ...
p Time.at(-0.5).nsec           # ... plus a positive remainder

# Time includes Comparable, so the whole ordering surface follows from <=>.
p Time.at(5) < Time.at(6)
p Time.at(5).between?(Time.at(1), Time.at(9))
p Time.at(1).clamp(Time.at(2), Time.at(5)).to_i
p [Time.at(3), Time.at(1), Time.at(2)].sort.map(&:to_i)
p Time.at(5) == Time.at(5)
p Time.at(5).getutc == Time.at(5)   # equality is by instant, not by zone
p({ Time.at(99) => "found" }[Time.at(99)])

# The civil constructors. `Time.utc`'s 7th argument is MICROSECONDS...
u = Time.utc(2007, 11, 1, 15, 25, 0, 123456)
p u.usec
p u.nsec
puts u.inspect
# ...while `Time.new`'s is a UTC OFFSET in seconds.
o = Time.new(2000, 1, 1, 0, 0, 0, 3600)
p o.utc_offset
p o.utc?
puts o.to_s

# --- Dir and File, on a real scratch tree ----------------------------------
# (`Dir.mktmpdir`/`Dir.tmpdir` are stdlib's tmpdir.rb, not core -- built by
# hand here so the example needs no require. The pid keeps concurrent runs
# from colliding.)
root = File.join(ENV.fetch("TMPDIR", "/tmp"), "zeo_core_classes_#{Process.pid}")
Dir.mkdir(root)
begin
  Dir.chdir(root) do
    Dir.mkdir("sub")
    File.write("one.rb", "alpha\nbeta\n")
    File.write("two.txt", "")
    File.write("sub/three.rb", "gamma\n")

    p File.read("one.rb")
    p File.readlines("one.rb")
    p File.readlines("one.rb", chomp: true)
    p File.size("one.rb")
    p File.exist?("one.rb"), File.exist?("nope.rb")
    p File.file?("one.rb"), File.file?("sub")
    p File.directory?("sub"), File.directory?("one.rb")
    p File.zero?("two.txt"), File.zero?("one.rb")
    p File.size?("two.txt")   # nil for an EMPTY file, not 0
    p File.size?("one.rb")

    # Dir listing and globbing.
    p Dir.children(".").sort
    p Dir.entries(".").sort
    p Dir.glob("*.rb").sort
    p Dir.glob("**/*.rb").sort
    p Dir.glob("*.{rb,txt}").sort
    p Dir.glob("?ne.rb")
    p Dir["*.rb"]
    p Dir.exist?("sub"), Dir.exist?("nope")
    p Dir.empty?("sub")

    # File.open: the block form closes afterwards and answers the block's
    # value; a lengthed read at EOF is nil where a whole-rest read is "".
    File.open("counted.txt", "w") { |f| f.print "ABCDEFGHIJ" }
    File.open("counted.txt", "r") do |f|
      p f.read(5)
      p f.read(5)
      p f.read(5)      # nil at EOF
      f.rewind
      p f.read(2)
      p f.tell
      f.seek(0)
      p f.read
      p f.eof?
    end
    p File.open("counted.txt", "r") { |f| f.read(3) }

    # A closed file raises rather than reading garbage.
    h = File.open("counted.txt", "r")
    h.close
    p h.closed?
    begin
      h.read
    rescue IOError => e
      puts "IOError: #{e.message}"
    end

    File.delete("two.txt")
    p File.exist?("two.txt")
    File.rename("one.rb", "renamed.rb")
    p File.exist?("renamed.rb")

    # Clean the tree from the inside, so the ensure below only has to remove
    # the (now empty) root.
    Dir.glob("**/*").sort.reverse.each do |e|
      File.directory?(e) ? Dir.rmdir(e) : File.delete(e)
    end
  end
ensure
  Dir.rmdir(root) if Dir.exist?(root)
end

# --- Errors: the Errno family, with CRuby's message shape ------------------
begin
  File.read("/definitely/not/here")
rescue Errno::ENOENT => e
  puts "#{e.class}: #{e.message}"
end
p Errno::ENOENT.superclass
p Errno::ENOENT.ancestors.include?(StandardError)

begin
  Dir.entries("/definitely/not/here")
rescue SystemCallError => e     # caught by the PARENT class
  puts e.class
end

# --- ENV: an Object with Hash-shaped methods, not a Hash -------------------
p ENV.class
ENV["ZEO_EXAMPLE"] = "set"
p ENV["ZEO_EXAMPLE"]
p ENV.key?("ZEO_EXAMPLE")
p ENV.fetch("ZEO_EXAMPLE")
p ENV.fetch("ZEO_ABSENT", "default")
p ENV.fetch("ZEO_ABSENT") { |k| "computed:#{k}" }
p ENV["ZEO_ABSENT"]
p ENV.delete("ZEO_EXAMPLE")
p ENV.key?("ZEO_EXAMPLE")
p ENV.to_h.class
p ENV.keys.class

# --- Process ---------------------------------------------------------------
p Process.pid.is_a?(Integer)
p Process.pid > 0
p Process.clock_gettime(Process::CLOCK_MONOTONIC).is_a?(Float)
p Process.clock_gettime(Process::CLOCK_MONOTONIC, :millisecond).is_a?(Integer)

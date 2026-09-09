require "tmpdir"
ZTMP = Dir.mktmpdir

root = File.join(ZTMP, "spinel_dir_entries_t")
Dir.mkdir(root) unless Dir.exist?(root)
File.write("#{root}/b.txt", "x")
File.write("#{root}/a.txt", "x")
File.write("#{root}/.hidden", "x")
puts Dir.entries(root).sort.inspect
puts Dir.children(root).sort.inspect
begin
  Dir.entries("#{root}/nope")
rescue => e
  # The message names the path, which is a fresh scratch directory on every
  # run: print the class and the tail instead.
  puts "raised: #{e.class} #{e.message[/[^\/]+\/nope\z/]}"
end
File.delete("#{root}/a.txt"); File.delete("#{root}/b.txt"); File.delete("#{root}/.hidden")
Dir.rmdir(root)
__END__
[".", "..", ".hidden", "a.txt", "b.txt"]
[".hidden", "a.txt", "b.txt"]
raised: Errno::ENOENT spinel_dir_entries_t/nope

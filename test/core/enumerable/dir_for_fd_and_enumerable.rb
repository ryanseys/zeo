# `Dir.for_fd` is not implemented, so the program dies on line 9 before any of
# the Enumerable-on-Dir coverage below it runs. ruby opens a Dir handle from a
# raw file descriptor.
# Dir.for_fd's handle lists through the descriptor (it has no path), and Dir's
# Enumerable surface routes through #entries.
d = "spinel_dir_suite"
Dir.mkdir(d) unless Dir.exist?(d)
File.write(File.join(d, "a.txt"), "a")
File.write(File.join(d, "b.txt"), "b")

h = Dir.new(d)
w = Dir.for_fd(h.fileno)
p w.path
p w.children.sort
p w.entries.sort

n = Dir.new(d)
p n.to_a.sort
p n.select { |e| e.end_with?(".txt") }.sort
p n.sort
p n.count
p n.include?("a.txt")
p n.map { |e| e.upcase }.sort
p n.reject { |e| e.start_with?(".") }.sort
n.close
h.close

p Dir.new(d).path
File.delete(File.join(d, "a.txt"))
File.delete(File.join(d, "b.txt"))
Dir.rmdir(d)
__END__
nil
["a.txt", "b.txt"]
[".", "..", "a.txt", "b.txt"]
[".", "..", "a.txt", "b.txt"]
["a.txt", "b.txt"]
[".", "..", "a.txt", "b.txt"]
4
true
[".", "..", "A.TXT", "B.TXT"]
["a.txt", "b.txt"]
"spinel_dir_suite"

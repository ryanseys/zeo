# File.readlines: read a file back as an array of lines, keeping the
# "\n" terminator (CRuby default). chomp: true strips it; an encoding:
# keyword is accepted and ignored. Uses a cwd-relative path because
# Windows MinGW builds can't fopen /tmp paths.
path = "file_readlines_tmp.txt"
File.write(path, "alpha\nbeta\ngamma\n")
p File.readlines(path)
p File.readlines(path, chomp: true)
p File.readlines(path, encoding: 'utf-8')

# A file whose last line has no trailing newline keeps that line bare.
File.write(path, "x\ny")
p File.readlines(path)

File.delete(path)

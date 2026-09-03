# Line-by-line file processing with File.open block
# Tests: File.open, each_line, string split, Hash

# Write test data: word frequency file
words = ["the", "quick", "brown", "fox", "jumps", "over", "lazy", "dog"]
File.open("/tmp/spinel_io_wf.txt", "w") do |f|
  i = 0
  while i < 5000
    w = words[i % 8]
    f.puts(w + " " + (i % 100).to_s)
    i = i + 1
  end
end

# Process: count word frequencies
counts = {}
File.open("/tmp/spinel_io_wf.txt", "r") do |f|
  f.each_line do |line|
    parts = line.strip.split(" ")
    if parts.length >= 1
      word = parts[0]
      if counts.has_key?(word)
        counts[word] = counts[word] + 1
      else
        counts[word] = 1
      end
    end
  end
end

# Print sorted results
counts.each do |k, v|
  puts k + ": " + v.to_s
end

# Process 20 times for benchmark
i = 0
while i < 20
  c = {}
  File.open("/tmp/spinel_io_wf.txt", "r") do |f|
    f.each_line do |line|
      parts = line.strip.split(" ")
      if parts.length >= 1
        w = parts[0]
        if c.has_key?(w)
          c[w] = c[w] + 1
        else
          c[w] = 1
        end
      end
    end
  end
  i = i + 1
end

File.delete("/tmp/spinel_io_wf.txt")
puts "done"
__END__
the: 625
quick: 625
brown: 625
fox: 625
jumps: 625
over: 625
lazy: 625
dog: 625
done

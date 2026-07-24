counts = Hash.new(0)
counts["the"] = 3
counts["dog"] = 1
ranked = counts.sort_by { |w, n| [-n, w] }
ranked.first(2).each { |w, n| puts "#{w}: #{n}" }
